//! Implements the Tauri desktop shell bindings that supervise the bundled
//! runtime and expose operator-facing diagnostics to the UI.

mod runtime_events;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rfd::FileDialog;
use runtime_config::{
    HostPlatform, RuntimeConfig, RUNTIME_ROOT_ENV,
};
use runtime_core::{
    emit_runtime_event,
    load_runtime_automation_snapshot,
    persist_runtime_automation_mode,
    read_health_report, read_supervision_contract, read_supervisor_snapshot,
    RuntimeAutomationMode, RuntimeAutomationSnapshot, RuntimeEventInput,
    RuntimeHealthReport, RuntimeRestartPolicy, RuntimeSupervisorSnapshot,
    RuntimeStatus, RuntimeSupervisorStatus,
};
use runtime_git::{
    assess_repository_access as assess_git_repository_access,
    detect_repository_provider_from_url as detect_git_repository_provider,
    RepositoryAccessAssessment,
    RepositoryProviderDetection,
    KIND_GIT_HTTP_BASIC, KIND_GIT_HTTP_BEARER,
    KIND_GIT_HTTP_GITHUB_HOST_LOGIN,
};
use runtime_runner::{
    RunnerFamily,
    unity::{
        default_unity_discovery_root_paths,
        diagnose_host_native_runner_config,
        inspect_host_capability_profile, HostCapabilityProfile,
        HostNativeRunnerDiagnostics,
    },
};
use runtime_store::{
    ArtifactInspectionRecord, AutomationSnapshot, BuildHistoryRecord,
    CredentialRecord,
    CreateRepositoryProjectBuildTargetInput,
    CreateRepositoryProjectPublishBindingInput as StoreCreateRepositoryProjectPublishBindingInput,
    CreateRepositoryProjectPublishTargetInput as StoreCreateRepositoryProjectPublishTargetInput,
    CreateRepositoryProjectInput as StoreCreateRepositoryProjectInput,
    OnDemandReleaseDispatchInput as StoreOnDemandReleaseDispatchInput,
    CreatedRepositoryProjectRecord,
    RepositoryProjectRecord as StoreRepositoryProjectRecord,
    RemoveRepositoryProjectInput as StoreRemoveRepositoryProjectInput,
    RemoveRepositoryProjectReport as StoreRemoveRepositoryProjectReport,
    ReleaseRunRecord,
    ReleaseSourceMetadata,
    RemoveRepositoryProjectStrategy,
    RepositoryAutomationStatus as StoreRepositoryAutomationStatus,
    UpdateRepositoryAuthStateInput as StoreUpdateRepositoryAuthStateInput,
    UpdateRepositoryProjectBuildTargetInput as StoreUpdateRepositoryProjectBuildTargetInput,
    UpdateRepositoryProjectPublishBindingInput as StoreUpdateRepositoryProjectPublishBindingInput,
    UpdateRepositoryProjectPublishTargetInput as StoreUpdateRepositoryProjectPublishTargetInput,
    UpdateRepositoryProjectInput as StoreUpdateRepositoryProjectInput,
    RuntimeControlRequest,
    ProcessFeedPage, ReleaseAutomationStatus, UpsertCredentialRecordInput,
    enqueue_runtime_control_request,
    initialize_database, list_artifact_inspection_records,
    list_build_history_records, list_process_feed_page,
    list_build_target_runtime_settings, list_credential_records,
    list_publish_target_binding_runtime_settings,
    list_publish_target_runtime_settings, LocalCoordinator, StorageLayout,
    KIND_ITCH_API_KEY,
};
use runtime_events::start_runtime_event_bridge;
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};
use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent,
    WebviewWindow, WindowEvent,
};
use zip::ZipArchive;

const RUNTIME_PACKAGE_NAME: &str = "runtime-bin";
const RUNTIME_BINARY_NAME: &str = "hgp-runtime";
const BUTLER_BINARY_NAME: &str = "hgp-butler";
const HGP_BUTLER_PATH_ENV: &str = "HGP_BUTLER_PATH";
const DEFAULT_RUNTIME_LOG_LINE_LIMIT: usize = 100;
const MAX_RUNTIME_LOG_LINE_LIMIT: usize = 500;
const DEFAULT_TEXT_FILE_PREVIEW_MAX_BYTES: usize = 128 * 1024;
const MAX_TEXT_FILE_PREVIEW_MAX_BYTES: usize = 512 * 1024;
const SECRET_STORAGE_MODEL_INLINE_SQLITE: &str =
    "sqlite-config-json-and-keyring-references";
const RUNTIME_STARTUP_PROBE_MILLIS: u64 = 150;
const RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS: u64 = 100;
const RUNTIME_SHUTDOWN_WAIT_POLLS: usize = 20;
const BUILD_EXECUTION_RETAINED_DIR_NAME: &str = "retained";
const BUILD_EXECUTION_REPORT_FILE_NAME: &str = "execution-report.json";
const BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME: &str = "execution-logs.zip";
const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ICON_ID: &str = "hgp-tray";
const TRAY_MENU_OPEN_ID: &str = "tray-open";
const TRAY_MENU_QUIT_ID: &str = "tray-quit";
const POPUP_WINDOW_WIDTH: u32 = 360;
const POPUP_WINDOW_HEIGHT: u32 = 420;
const FOCUS_WINDOW_WIDTH: u32 = POPUP_WINDOW_WIDTH + (POPUP_WINDOW_WIDTH / 2);
const FOCUS_WINDOW_HEIGHT: u32 = POPUP_WINDOW_HEIGHT * 2;
const POPUP_WINDOW_MIN_WIDTH: u32 = 360;
const POPUP_WINDOW_MIN_HEIGHT: u32 = 420;
const POPUP_WINDOW_EDGE_MARGIN: i32 = 16;
const WINDOW_FOCUS_TRANSITION_MILLIS: u64 = 150;
const WINDOW_FOCUS_TRANSITION_STEP_MILLIS: u64 = 15;
const SYSTEM_DIALOG_FOCUS_LOSS_GRACE: Duration = Duration::from_millis(250);
const DEFAULT_PROCESS_FEED_PAGE_SIZE: u32 = 6;
const MAX_PROCESS_FEED_PAGE_SIZE: u32 = 50;
const HOST_CAPABILITY_PROFILE_CACHE_TTL: Duration = Duration::from_secs(30);
const DEFAULT_HOST_NATIVE_RUNNER_TYPE: &str = "host-native";
const DEFAULT_BUILD_TARGET_TIMEOUT_SECONDS: i64 = 3600;
const MIN_REPOSITORY_POLL_INTERVAL_SECONDS: i64 = 5;
const SUPPORTED_REPOSITORY_ENGINE_KIND_UNITY: &str = "unity";
const GITHUB_AUTH_PROVIDER_ID: &str = "github";
const EVENT_TOPIC_AUTOMATION_MODE_CHANGED: &str = "automation.mode_changed";
const EVENT_TOPIC_RELEASE_QUEUED: &str = "automation.release_queued";
const GITHUB_AUTH_PROVIDER_LABEL: &str = "GitHub";
const GITHUB_AUTH_INSTANCE_URL: &str = "https://github.com";
const GITHUB_AUTH_CREDENTIAL_NAME: &str = "GitHub.com";
const GITHUB_AUTH_CREDENTIAL_HELPER: &str = "manager";
const GITHUB_AUTH_MODE_BROWSER: &str = "browser";
const AUTH_PROVIDER_STATUS_CONNECTED: &str = "connected";
const AUTH_PROVIDER_STATUS_DISCONNECTED: &str = "disconnected";
const AUTH_PROVIDER_STATUS_UNAVAILABLE: &str = "unavailable";
const DESKTOP_SHELL_RERUN_REQUESTED_VIA: &str = "desktop-shell-ui";
const LOCALIZATION_RESOURCE_DIR_NAME: &str = "localizations";
const LOCALIZATION_SETTINGS_FILE_NAME: &str = "localization-settings.json";
const DEFAULT_PRIMARY_LOCALE_CODE: &str = "en";
const DEFAULT_FALLBACK_LOCALE_CODE: &str = "pt-BR";
const OFFICIAL_LOCALE_CODES: &[&str] = &["en", "pt-BR"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowLayoutPreset {
    width: u32,
    height: u32,
}

impl WindowLayoutPreset {
    const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowTransitionSettings {
    main: WindowLayoutPreset,
    focus: WindowLayoutPreset,
    duration_millis: u64,
}

impl WindowTransitionSettings {
    const fn current() -> Self {
        Self {
            main: WindowLayoutPreset::new(
                POPUP_WINDOW_WIDTH,
                POPUP_WINDOW_HEIGHT,
            ),
            focus: WindowLayoutPreset::new(
                FOCUS_WINDOW_WIDTH,
                FOCUS_WINDOW_HEIGHT,
            ),
            duration_millis: WINDOW_FOCUS_TRANSITION_MILLIS,
        }
    }

    fn animation_steps(self) -> u32 {
        let steps = self.duration_millis / WINDOW_FOCUS_TRANSITION_STEP_MILLIS;
        steps.max(1) as u32
    }

    fn target_layout(self, target: WindowFocusTarget) -> WindowLayoutPreset {
        match target {
            WindowFocusTarget::Main => self.main,
            WindowFocusTarget::Focus => self.focus,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowFocusTarget {
    Main,
    Focus,
}

impl WindowFocusTarget {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "main" => Ok(Self::Main),
            "focus" => Ok(Self::Focus),
            other => Err(format!(
                "unsupported window focus target {:?}; expected \"main\" or \"focus\"",
                other,
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeLaunchAction {
    Supervise,
    Shutdown,
}

impl RuntimeLaunchAction {
    const fn as_arg(self) -> &'static str {
        match self {
            Self::Supervise => "supervise",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeCommandPlan {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    inherit_stdio: bool,
}

impl RuntimeCommandPlan {
    fn into_command(self) -> Command {
        let mut command = Command::new(self.program);
        command.args(self.args);
        command.stdin(Stdio::null());
        if let Some(current_dir) = self.current_dir {
            command.current_dir(current_dir);
        }
        if !self.inherit_stdio {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }
        command
    }
}

#[derive(Default)]
struct RuntimeProcessState {
    child: Mutex<Option<Child>>,
}

#[derive(Default)]
struct ShellLifecycleState {
    is_quitting: Mutex<bool>,
    is_main_window_pinned: Mutex<bool>,
    active_system_dialogs: Mutex<u32>,
    suppress_main_window_focus_loss_until: Mutex<Option<Instant>>,
}

#[derive(Debug, Clone)]
struct CachedHostCapabilityProfile {
    cached_at: Instant,
    profile: HostCapabilityProfile,
}

static HOST_CAPABILITY_PROFILE_CACHE: LazyLock<
    Mutex<HashMap<&'static str, CachedHostCapabilityProfile>>,
> = LazyLock::new(|| Mutex::new(HashMap::new()));

struct RepositoryInspectionResources {
    generated_at: String,
    release_status_by_repository: HashMap<i64, StoreRepositoryAutomationStatus>,
    credential_by_id: HashMap<i64, RepositoryCredentialReference>,
    build_targets_by_repository: HashMap<i64, Vec<UnityAdapterBuildTargetSettings>>,
    publish_targets_by_repository: HashMap<i64, Vec<RepositoryPublishTargetInspection>>,
}

struct ActiveSystemDialogGuard<'a> {
    lifecycle: &'a ShellLifecycleState,
}

impl<'a> ActiveSystemDialogGuard<'a> {
    fn acquire(lifecycle: &'a ShellLifecycleState) -> Result<Self, String> {
        let mut active_system_dialogs = lifecycle
            .active_system_dialogs
            .lock()
            .map_err(|_| "system dialog state is unavailable".to_string())?;
        *active_system_dialogs = active_system_dialogs.saturating_add(1);
        Ok(Self { lifecycle })
    }
}

impl Drop for ActiveSystemDialogGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active_system_dialogs) = self.lifecycle.active_system_dialogs.lock() {
            *active_system_dialogs = active_system_dialogs.saturating_sub(1);

            if *active_system_dialogs == 0 {
                suppress_main_window_focus_loss(
                    self.lifecycle,
                    SYSTEM_DIALOG_FOCUS_LOSS_GRACE,
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeDirectorySettings {
    data_dir: PathBuf,
    state_dir: PathBuf,
    logs_dir: PathBuf,
    artifacts_dir: PathBuf,
    runs_dir: PathBuf,
    database_path: PathBuf,
    health_report_path: PathBuf,
    supervision_contract_path: PathBuf,
    supervisor_state_path: PathBuf,
    runtime_events_path: PathBuf,
    runtime_events_cursor_path: PathBuf,
    runtime_log_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalizationLocaleSettings {
    code: String,
    display_name: String,
    native_name: String,
    message_count: usize,
    is_official: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LocalizationSettings {
    localization_root: PathBuf,
    primary_locale: String,
    fallback_locale: String,
    available_locales: Vec<LocalizationLocaleSettings>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SaveLocalizationPreferencesInput {
    primary_locale: String,
    fallback_locale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PersistedLocalizationPreferences {
    primary_locale: String,
    fallback_locale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct LocalizationPackDocument {
    display_name: String,
    #[serde(default)]
    native_name: String,
    #[serde(default)]
    messages: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityDiscoveryRootSetting {
    path: PathBuf,
    exists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityAdapterBuildTargetSettings {
    build_target_id: i64,
    repository_id: i64,
    repository_name: String,
    target_name: String,
    unity_target_platform: String,
    runner_type: String,
    unity_build_method: Option<String>,
    enabled: bool,
    diagnostic_status: String,
    diagnostic_message: String,
    host_native_diagnostics: Option<HostNativeRunnerDiagnostics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct UnityAdapterSettings {
    platform: String,
    supported_runner_families: Vec<String>,
    discovery_roots: Vec<UnityDiscoveryRootSetting>,
    capability_profile: HostCapabilityProfile,
    build_targets: Vec<UnityAdapterBuildTargetSettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretBindingReference {
    binding_kind: String,
    binding_id: i64,
    binding_name: String,
    repository_id: i64,
    repository_name: String,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositorySecretBindingSetting {
    repository_id: i64,
    repository_name: String,
    credentials_id: Option<i64>,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct PublishTargetSecretBindingSetting {
    publish_target_id: i64,
    repository_id: i64,
    repository_name: String,
    publish_target_name: String,
    publish_target_kind: String,
    credentials_id: Option<i64>,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct CredentialConfigSummary {
    status: String,
    message: String,
    top_level_keys: Vec<String>,
    missing_required_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretCredentialSetting {
    credential_id: i64,
    name: String,
    kind: String,
    created_at: String,
    updated_at: String,
    storage_model: String,
    config_summary: CredentialConfigSummary,
    bindings: Vec<SecretBindingReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SecretSettings {
    storage_model: String,
    supported_credential_kinds: Vec<String>,
    warnings: Vec<String>,
    credentials: Vec<SecretCredentialSetting>,
    repository_bindings: Vec<RepositorySecretBindingSetting>,
    publish_target_bindings: Vec<PublishTargetSecretBindingSetting>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SaveSecretCredentialInput {
    credential_id: Option<i64>,
    name: String,
    kind: String,
    config_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ConnectRepositoryAuthInput {
    repository_id: i64,
    credentials_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ReconnectRepositoryAuthInput {
    repository_id: i64,
    credentials_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct DisconnectRepositoryAuthInput {
    repository_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct SyncRepositoryAuthAssessmentInput {
    repository_id: i64,
    repository_access_assessment: RepositoryAccessAssessment,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdatePublishTargetSecretBindingInput {
    publish_target_id: i64,
    credentials_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RepositoryAccessAssessmentInput {
    repository_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct AuthProviderStatus {
    provider_id: String,
    label: String,
    status: String,
    status_message: String,
    instance_url: String,
    credential_id: Option<i64>,
    credential_name: Option<String>,
    credential_created_at: Option<String>,
    credential_updated_at: Option<String>,
    bound_repository_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UnityBuildContractCommandInput {
    target_platform: String,
    build_method: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct BuildContractCommandInput {
    unity: Option<UnityBuildContractCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CreateRepositoryProjectBuildTargetCommandInput {
    name: String,
    contract: BuildContractCommandInput,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CreateRepositoryProjectPublishBindingCommandInput {
    build_target_name: String,
    enabled: bool,
    options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CreateRepositoryProjectPublishTargetCommandInput {
    name: String,
    kind: String,
    enabled: bool,
    config_json: String,
    credentials_id: Option<i64>,
    #[serde(default)]
    bindings: Vec<CreateRepositoryProjectPublishBindingCommandInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostPathSelectionKind {
    File,
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PickHostPathFilterInput {
    name: String,
    extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PickHostPathInput {
    kind: HostPathSelectionKind,
    title: Option<String>,
    #[serde(default)]
    filters: Vec<PickHostPathFilterInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct CreateRepositoryProjectCommandInput {
    name: String,
    engine_kind: String,
    source_mode: Option<String>,
    repository_url: Option<String>,
    local_path: Option<String>,
    repository_access_assessment: Option<RepositoryAccessAssessment>,
    repository_credentials_id: Option<i64>,
    personal_access_token: Option<String>,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    build_targets: Vec<CreateRepositoryProjectBuildTargetCommandInput>,
    #[serde(default)]
    publish_targets: Vec<CreateRepositoryProjectPublishTargetCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectBuildTargetCommandInput {
    build_target_id: Option<i64>,
    name: String,
    contract: BuildContractCommandInput,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectPublishBindingCommandInput {
    build_target_id: Option<i64>,
    build_target_name: String,
    enabled: bool,
    options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectPublishTargetCommandInput {
    publish_target_id: Option<i64>,
    name: String,
    kind: String,
    enabled: bool,
    config_json: String,
    credentials_id: Option<i64>,
    #[serde(default)]
    bindings: Vec<UpdateRepositoryProjectPublishBindingCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectCommandInput {
    repository_id: i64,
    name: String,
    engine_kind: String,
    source_mode: String,
    repository_url: Option<String>,
    local_path: Option<String>,
    repository_access_assessment: Option<RepositoryAccessAssessment>,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    enabled: bool,
    build_targets: Vec<UpdateRepositoryProjectBuildTargetCommandInput>,
    #[serde(default)]
    publish_targets: Vec<UpdateRepositoryProjectPublishTargetCommandInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
struct RemoveRepositoryProjectCommandInput {
    repository_id: i64,
    strategy: RemoveRepositoryProjectStrategy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RepositoryInstantCheckInput {
    repository_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct OnDemandReleaseProcessCommandInput {
    repository_id: i64,
    release_version: Option<String>,
    version_source: String,
    source_kind: String,
    source_ref: Option<String>,
    local_path: Option<String>,
    unity_executable_path_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectBuildTargetCommandInput {
    name: String,
    target_platform: String,
    build_method: String,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectPublishBindingCommandInput {
    build_target_name: String,
    enabled: bool,
    options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectPublishTargetCommandInput {
    name: String,
    kind: String,
    enabled: bool,
    config_json: String,
    credentials_id: Option<i64>,
    bindings: Vec<NormalizedCreateRepositoryProjectPublishBindingCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectCommandInput {
    name: String,
    engine_kind: String,
    source_mode: String,
    repository_url: Option<String>,
    local_path: Option<String>,
    repository_access_assessment: Option<RepositoryAccessAssessment>,
    repository_credentials_id: Option<i64>,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    build_targets: Vec<NormalizedCreateRepositoryProjectBuildTargetCommandInput>,
    publish_targets: Vec<NormalizedCreateRepositoryProjectPublishTargetCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedUpdateRepositoryProjectBuildTargetCommandInput {
    build_target_id: Option<i64>,
    name: String,
    target_platform: String,
    build_method: String,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedUpdateRepositoryProjectPublishBindingCommandInput {
    build_target_id: Option<i64>,
    build_target_name: String,
    enabled: bool,
    options_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedUpdateRepositoryProjectPublishTargetCommandInput {
    publish_target_id: Option<i64>,
    name: String,
    kind: String,
    enabled: bool,
    config_json: String,
    credentials_id: Option<i64>,
    bindings: Vec<NormalizedUpdateRepositoryProjectPublishBindingCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedUpdateRepositoryProjectCommandInput {
    repository_id: i64,
    name: String,
    engine_kind: String,
    source_mode: String,
    repository_url: Option<String>,
    local_path: Option<String>,
    repository_access_assessment: Option<RepositoryAccessAssessment>,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    enabled: bool,
    build_targets: Vec<NormalizedUpdateRepositoryProjectBuildTargetCommandInput>,
    publish_targets: Vec<NormalizedUpdateRepositoryProjectPublishTargetCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
struct ProcessFeedInput {
    page: Option<u32>,
    page_size: Option<u32>,
}

impl ProcessFeedInput {
    fn normalized(&self) -> (u32, u32) {
        (
            self.page.unwrap_or(1).max(1),
            self.page_size
                .unwrap_or(DEFAULT_PROCESS_FEED_PAGE_SIZE)
                .clamp(1, MAX_PROCESS_FEED_PAGE_SIZE),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeLifecycleCommandSettings {
    program: PathBuf,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeLifecycleSettings {
    startup_command: RuntimeLifecycleCommandSettings,
    shutdown_command: RuntimeLifecycleCommandSettings,
    shell_launches_supervisor_on_startup: bool,
    shell_requests_shutdown_on_exit: bool,
    shell_force_kills_after_timeout: bool,
    shell_relaunches_supervisor_on_restart: bool,
    runtime_supervisor_owns_crash_recovery: bool,
    shutdown_grace_period_millis: u64,
    restart_policy: RuntimeRestartPolicy,
    crash_recovery_status: String,
    supervisor_snapshot: Option<RuntimeSupervisorSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryCredentialReference {
    credential_id: i64,
    name: String,
    kind: String,
    config_status: String,
    config_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryPublishBindingInspection {
    build_target_id: i64,
    build_target_name: String,
    enabled: bool,
    options_json: String,
    consumption_behavior: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryPublishTargetInspection {
    publish_target_id: i64,
    name: String,
    kind: String,
    enabled: bool,
    config_json: String,
    credentials: Option<RepositoryCredentialReference>,
    bindings: Vec<RepositoryPublishBindingInspection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryInspectionEntry {
    repository_id: i64,
    repository_name: String,
    source_mode: String,
    workspace_strategy: String,
    repo_url: String,
    local_path: Option<String>,
    engine_kind: String,
    enabled: bool,
    polling_interval_seconds: i64,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    last_seen_tag: Option<String>,
    enabled_build_target_count: i64,
    credentials: Option<RepositoryCredentialReference>,
    source_provider_id: Option<String>,
    source_instance_url: Option<String>,
    visibility_status: String,
    auth_requirement_status: String,
    auth_binding_status: String,
    auth_status_message: String,
    auth_last_verified_at: Option<String>,
    build_targets: Vec<UnityAdapterBuildTargetSettings>,
    publish_targets: Vec<RepositoryPublishTargetInspection>,
    pending_release_count: i64,
    queued_build_runs: i64,
    running_build_runs: i64,
    queued_publish_runs: i64,
    running_publish_runs: i64,
    release_queue: Vec<ReleaseAutomationStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryInspectionSettings {
    generated_at: String,
    repositories: Vec<RepositoryInspectionEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RetainedLogArchiveEntry {
    entry_path: String,
    entry_name: String,
    size_bytes: u64,
    compressed_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct BuildExecutionReportPayload {
    build_run_id: i64,
    workspace_path: Option<PathBuf>,
    retained_dir_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    logs_archive_path: Option<PathBuf>,
    exists: bool,
    logs_archive_exists: bool,
    log_entries: Vec<RetainedLogArchiveEntry>,
    report: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct BuildExecutionRetentionPurgeReport {
    build_run_id: i64,
    workspace_path: Option<PathBuf>,
    retained_dir_path: Option<PathBuf>,
    removed_paths: Vec<PathBuf>,
    workspace_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct HostTextFilePayload {
    path: PathBuf,
    exists: bool,
    size_bytes: u64,
    truncated: bool,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RetainedLogArchiveEntryPreviewPayload {
    archive_path: PathBuf,
    entry_path: String,
    exists: bool,
    size_bytes: u64,
    truncated: bool,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReleaseProcessOutputsDeleteReport {
    release_run_id: i64,
    artifact_root_path: Option<PathBuf>,
    removed_paths: Vec<PathBuf>,
    missing_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct BuildLogDeleteReport {
    build_run_id: i64,
    log_path: Option<PathBuf>,
    removed_paths: Vec<PathBuf>,
    missing_paths: Vec<PathBuf>,
    parent_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryProjectDeleteReport {
    repository_id: i64,
    repository_name: String,
    strategy: RemoveRepositoryProjectStrategy,
    release_run_count: u64,
    build_run_count: u64,
    publish_run_count: u64,
    queue_message_count: u64,
    coordination_lease_count: u64,
    idempotency_key_count: u64,
    removed_paths: Vec<PathBuf>,
    missing_paths: Vec<PathBuf>,
    skipped_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ApplicationVersionInfo {
    product_name: String,
    app_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct NormalizedRepositoryProjectRemovalPaths {
    directory_paths: Vec<PathBuf>,
    file_paths: Vec<PathBuf>,
}

fn main_window(app_handle: &AppHandle) -> Result<WebviewWindow, String> {
    app_handle
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "main window handle is unavailable".to_string())
}

fn is_main_window_pinned(app_handle: &AppHandle) -> bool {
    let lifecycle = app_handle.state::<ShellLifecycleState>();

    lifecycle
        .is_main_window_pinned
        .lock()
        .map(|is_pinned| *is_pinned)
        .unwrap_or(false)
}

fn set_main_window_pinned_state(app_handle: &AppHandle, pinned: bool) -> Result<bool, String> {
    let lifecycle = app_handle.state::<ShellLifecycleState>();
    let mut is_pinned = lifecycle
        .is_main_window_pinned
        .lock()
        .map_err(|_| "main window pin state is unavailable".to_string())?;
    *is_pinned = pinned;
    Ok(*is_pinned)
}

fn has_active_system_dialogs(lifecycle: &ShellLifecycleState) -> bool {
    lifecycle
        .active_system_dialogs
        .lock()
        .map(|active_system_dialogs| *active_system_dialogs > 0)
        .unwrap_or(true)
}

fn suppress_main_window_focus_loss(
    lifecycle: &ShellLifecycleState,
    duration: Duration,
) {
    if let Ok(mut suppressed_until) = lifecycle.suppress_main_window_focus_loss_until.lock() {
        *suppressed_until = Some(Instant::now() + duration);
    }
}

fn is_main_window_focus_loss_suppressed(lifecycle: &ShellLifecycleState) -> bool {
    let now = Instant::now();

    lifecycle
        .suppress_main_window_focus_loss_until
        .lock()
        .map(|mut suppressed_until| match *suppressed_until {
            Some(until) if until > now => true,
            Some(_) => {
                *suppressed_until = None;
                false
            }
            None => false,
        })
        .unwrap_or(true)
}

fn should_hide_main_window_on_focus_loss_state(
    should_keep_running: bool,
    is_pinned: bool,
    has_active_system_dialogs: bool,
    is_focus_loss_suppressed: bool,
) -> bool {
    should_keep_running
        && !is_pinned
        && !has_active_system_dialogs
        && !is_focus_loss_suppressed
}

fn should_hide_main_window_on_focus_loss(app_handle: &AppHandle) -> bool {
    if !should_keep_running(app_handle) {
        return false;
    }

    let lifecycle = app_handle.state::<ShellLifecycleState>();
    let is_pinned = lifecycle
        .is_main_window_pinned
        .lock()
        .map(|is_pinned| *is_pinned)
        .unwrap_or(true);
    let has_active_system_dialogs = has_active_system_dialogs(&lifecycle);
    let is_focus_loss_suppressed = is_main_window_focus_loss_suppressed(&lifecycle);

    should_hide_main_window_on_focus_loss_state(
        true,
        is_pinned,
        has_active_system_dialogs,
        is_focus_loss_suppressed,
    )
}

fn initialize_tray(app: &tauri::App) -> Result<(), String> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_MENU_OPEN_ID, "Open HGP")
        .separator()
        .text(TRAY_MENU_QUIT_ID, "Quit")
        .build()
        .map_err(|error| error.to_string())?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default tray icon is unavailable".to_string())?;

    TrayIconBuilder::with_id(TRAY_ICON_ID)
        .icon(icon)
        .tooltip("HGP")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .build(app)
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn configure_main_window(app_handle: &AppHandle) -> Result<(), String> {
    let window = main_window(app_handle)?;
    let transition = window_transition_settings();
    window
        .set_resizable(false)
        .map_err(|error| error.to_string())?;
    window
        .set_minimizable(false)
        .map_err(|error| error.to_string())?;
    window
        .set_maximizable(false)
        .map_err(|error| error.to_string())?;
    window
        .set_always_on_top(true)
        .map_err(|error| error.to_string())?;
    window
        .set_skip_taskbar(true)
        .map_err(|error| error.to_string())?;
    window
        .set_min_size(Some(PhysicalSize::new(
            POPUP_WINDOW_MIN_WIDTH,
            POPUP_WINDOW_MIN_HEIGHT,
        )))
        .map_err(|error| error.to_string())?;
    apply_main_window_layout(app_handle, transition.main)
}

fn pin_main_window_to_primary_monitor(app_handle: &AppHandle) -> Result<(), String> {
    let window = main_window(app_handle)?;
    let outer_size = window.outer_size().map_err(|error| error.to_string())?;
    position_main_window(
        &window,
        WindowLayoutPreset::new(outer_size.width, outer_size.height),
    )
}

fn apply_main_window_layout(
    app_handle: &AppHandle,
    desired_layout: WindowLayoutPreset,
) -> Result<(), String> {
    let window = main_window(app_handle)?;
    let clamped_layout = clamp_window_layout(&window, desired_layout)?;

    window
        .set_size(PhysicalSize::new(
            clamped_layout.width,
            clamped_layout.height,
        ))
        .map_err(|error| error.to_string())?;

    let outer_size = window.outer_size().map_err(|error| error.to_string())?;
    position_main_window(
        &window,
        WindowLayoutPreset::new(outer_size.width, outer_size.height),
    )
}

fn clamp_window_layout(
    window: &WebviewWindow,
    desired_layout: WindowLayoutPreset,
) -> Result<WindowLayoutPreset, String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "primary monitor is unavailable".to_string())?;
    let work_area = monitor.work_area();

    Ok(WindowLayoutPreset::new(
        desired_layout.width.min(work_area.size.width),
        desired_layout.height.min(work_area.size.height),
    ))
}

fn position_main_window(
    window: &WebviewWindow,
    layout: WindowLayoutPreset,
) -> Result<(), String> {
    let monitor = window
        .primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "primary monitor is unavailable".to_string())?;
    let work_area = monitor.work_area();
    let width = layout.width.min(work_area.size.width);
    let height = layout.height.min(work_area.size.height);
    let x = work_area.position.x + work_area.size.width as i32
        - width as i32
        - POPUP_WINDOW_EDGE_MARGIN;
    let y = work_area.position.y + work_area.size.height as i32
        - height as i32
        - POPUP_WINDOW_EDGE_MARGIN;

    window
        .set_position(PhysicalPosition::new(
            x.max(work_area.position.x),
            y.max(work_area.position.y),
        ))
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn show_main_window(app_handle: &AppHandle) {
    if let Err(error) = pin_main_window_to_primary_monitor(app_handle) {
        eprintln!("failed to position main window: {error}");
    }

    match main_window(app_handle) {
        Ok(window) => {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        Err(error) => eprintln!("failed to show main window: {error}"),
    }
}

fn restore_main_window_focus_after_system_dialog(app_handle: &AppHandle) {
    if !should_keep_running(app_handle) {
        return;
    }

    match main_window(app_handle) {
        Ok(window) => {
            if window.is_visible().ok().unwrap_or(true) {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
        Err(error) => eprintln!("failed to restore main window focus: {error}"),
    }
}

fn hide_main_window(app_handle: &AppHandle) -> Result<(), String> {
    let window = main_window(app_handle)?;
    window.hide().map_err(|error| error.to_string())
}

fn request_app_exit(app_handle: &AppHandle) {
    if let Ok(mut is_quitting) = app_handle.state::<ShellLifecycleState>().is_quitting.lock() {
        *is_quitting = true;
    }
    app_handle.exit(0);
}

fn should_keep_running(app_handle: &AppHandle) -> bool {
    app_handle
        .state::<ShellLifecycleState>()
        .is_quitting
        .lock()
        .map(|is_quitting| !*is_quitting)
        .unwrap_or(false)
}

/// Runs the desktop shell and supervises the bundled local runtime process.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(RuntimeProcessState::default())
        .manage(ShellLifecycleState::default())
        .invoke_handler(tauri::generate_handler![
            application_version,
            process_feed,
            open_host_path,
            open_external_url,
            read_host_text_file,
            transition_window_focus,
            main_window_pin_state,
            set_main_window_pinned,
            close_main_window,
            pick_host_path,
            pick_unity_executable_path,
            validate_unity_executable_path,
            create_repository_project,
            update_repository_project,
            remove_repository_project,
            runtime_health,
            runtime_automation_status,
            runtime_logs,
            runtime_directories,
            localization_settings,
            save_localization_preferences,
            runtime_lifecycle_settings,
            release_status,
            repository_inspection,
            repository_project_detail,
            build_history,
            artifact_inspection,
            build_execution_report,
            read_retained_log_archive_entry,
            purge_build_execution_retention,
            delete_release_process_outputs,
            delete_build_log,
            detect_repository_provider,
            assess_repository_access,
            auth_providers,
            login_github_auth,
            secret_settings,
            save_secret_credential,
            connect_repository_auth,
            reconnect_repository_auth,
            disconnect_repository_auth,
            sync_repository_auth_assessment,
            update_publish_target_secret_binding,
            start_runtime,
            stop_runtime,
            restart_runtime,
            set_runtime_automation_mode,
            dispatch_on_demand_release_process,
            rerun_release_process,
            request_repository_instant_check,
            unity_adapter_settings,
        ])
        .setup(|app| {
            launch_runtime_process(app)
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;
            let config = load_shell_runtime_config()
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(error) })?;
            let storage = StorageLayout::from_directories(&config.directories);
            start_runtime_event_bridge(app.handle().clone(), storage);
            initialize_tray(app)
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(io::Error::other(error)) })?;
            configure_main_window(app.handle())
                .map_err(|error| -> Box<dyn std::error::Error> { Box::new(io::Error::other(error)) })?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("desktop shell failed to build");

    app.run(|app_handle, event| {
        match event {
            RunEvent::Ready => show_main_window(app_handle),
            RunEvent::WindowEvent { label, event, .. }
                if label == MAIN_WINDOW_LABEL
                    && matches!(event, WindowEvent::CloseRequested { .. }) =>
            {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    if should_keep_running(app_handle) {
                        api.prevent_close();
                        if let Err(error) = hide_main_window(app_handle) {
                            eprintln!("failed to hide main window: {error}");
                        }
                    }
                }
            }
            RunEvent::WindowEvent { label, event, .. }
                if label == MAIN_WINDOW_LABEL
                    && matches!(event, WindowEvent::Focused(false)) =>
            {
                if should_hide_main_window_on_focus_loss(app_handle) {
                    if let Err(error) = hide_main_window(app_handle) {
                        eprintln!("failed to hide main window after focus loss: {error}");
                    }
                }
            }
            RunEvent::MenuEvent(event) if event.id() == TRAY_MENU_OPEN_ID => {
                show_main_window(app_handle);
            }
            RunEvent::MenuEvent(event) if event.id() == TRAY_MENU_QUIT_ID => {
                request_app_exit(app_handle);
            }
            RunEvent::TrayIconEvent(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }) => {
                show_main_window(app_handle);
            }
            RunEvent::TrayIconEvent(TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }) => {
                show_main_window(app_handle);
            }
            RunEvent::Exit => stop_runtime_process(app_handle),
            _ => {}
        }
    });
}

#[tauri::command]
fn application_version(app_handle: AppHandle) -> Result<ApplicationVersionInfo, String> {
    let package = app_handle.package_info();
    Ok(ApplicationVersionInfo {
        product_name: package.name.clone(),
        app_version: package.version.to_string(),
    })
}

#[tauri::command]
fn process_feed(input: Option<ProcessFeedInput>) -> Result<ProcessFeedPage, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_process_feed(&config, input.unwrap_or_default()).map_err(|error| error.to_string())
}

#[tauri::command]
fn transition_window_focus(app_handle: AppHandle, target: String) -> Result<(), String> {
    animate_main_window_focus_transition(&app_handle, WindowFocusTarget::parse(&target)?)
}

#[tauri::command]
fn main_window_pin_state(app_handle: AppHandle) -> Result<bool, String> {
    Ok(is_main_window_pinned(&app_handle))
}

#[tauri::command]
fn set_main_window_pinned(app_handle: AppHandle, pinned: bool) -> Result<bool, String> {
    set_main_window_pinned_state(&app_handle, pinned)
}

#[tauri::command]
fn close_main_window(app_handle: AppHandle) -> Result<(), String> {
    if should_keep_running(&app_handle) {
        return hide_main_window(&app_handle);
    }

    request_app_exit(&app_handle);
    Ok(())
}

#[tauri::command]
fn pick_host_path(app_handle: AppHandle, input: PickHostPathInput) -> Result<Option<String>, String> {
    let mut dialog = FileDialog::new();
    let lifecycle = app_handle.state::<ShellLifecycleState>();
    let dialog_guard = ActiveSystemDialogGuard::acquire(&lifecycle)?;

    if let Some(title) = normalize_optional_shell_string(input.title) {
        dialog = dialog.set_title(&title);
    }

    if matches!(input.kind, HostPathSelectionKind::File) {
        for filter in input.filters {
            let filter_name = filter.name.trim();
            if filter_name.is_empty() || filter.extensions.is_empty() {
                continue;
            }

            let extensions: Vec<&str> =
                filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(filter_name, &extensions);
        }
    }

    let selected_path = match input.kind {
        HostPathSelectionKind::File => dialog.pick_file(),
        HostPathSelectionKind::Directory => dialog.pick_folder(),
    };

    restore_main_window_focus_after_system_dialog(&app_handle);
    drop(dialog_guard);

    Ok(selected_path.map(|path| path.display().to_string()))
}

#[tauri::command]
fn pick_unity_executable_path(app_handle: AppHandle) -> Result<Option<String>, String> {
    pick_host_path(app_handle, PickHostPathInput {
        kind: HostPathSelectionKind::File,
        title: Some("Select Unity Editor executable".to_string()),
        filters: vec![PickHostPathFilterInput {
            name: "Unity Editor".to_string(),
            extensions: vec!["exe".to_string(), "app".to_string()],
        }],
    })
}

#[tauri::command]
fn validate_unity_executable_path(path: String) -> Result<HostNativeRunnerDiagnostics, String> {
    Ok(validate_unity_executable_path_diagnostics(&path))
}

#[tauri::command]
fn create_repository_project(
    input: CreateRepositoryProjectCommandInput,
) -> Result<CreatedRepositoryProjectRecord, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_project(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_repository_project(
    input: UpdateRepositoryProjectCommandInput,
) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_project_update(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn remove_repository_project(
    input: RemoveRepositoryProjectCommandInput,
) -> Result<RepositoryProjectDeleteReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_project_removal(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_health() -> Result<RuntimeHealthReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_health_report(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_automation_status() -> Result<RuntimeAutomationSnapshot, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    let storage = StorageLayout::from_directories(&config.directories);
    load_runtime_automation_snapshot(&storage).map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_logs(line_limit: Option<usize>) -> Result<Vec<String>, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_log_lines(&config, normalize_runtime_log_line_limit(line_limit))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_directories() -> Result<RuntimeDirectorySettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_directory_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn localization_settings(app_handle: AppHandle) -> Result<LocalizationSettings, String> {
    let localization_root =
        resolve_localization_resource_root(&app_handle).map_err(|error| error.to_string())?;
    let settings_dir =
        resolve_localization_settings_dir(&app_handle).map_err(|error| error.to_string())?;

    load_localization_settings_from_paths(&localization_root, &settings_dir)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_localization_preferences(
    app_handle: AppHandle,
    input: SaveLocalizationPreferencesInput,
) -> Result<LocalizationSettings, String> {
    let localization_root =
        resolve_localization_resource_root(&app_handle).map_err(|error| error.to_string())?;
    let settings_dir =
        resolve_localization_settings_dir(&app_handle).map_err(|error| error.to_string())?;

    persist_localization_preferences_to_paths(
        &localization_root,
        &settings_dir,
        input,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn runtime_lifecycle_settings() -> Result<RuntimeLifecycleSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_lifecycle_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_host_path(path: String) -> Result<(), String> {
    open_path_in_host(Path::new(path.trim())).map_err(|error| error.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    open_url_in_host(url.trim()).map_err(|error| error.to_string())
}

#[tauri::command]
fn read_host_text_file(
    path: String,
    max_bytes: Option<usize>,
) -> Result<HostTextFilePayload, String> {
    load_host_text_file(
        Path::new(path.trim()),
        normalize_text_file_preview_max_bytes(max_bytes),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn release_status() -> Result<AutomationSnapshot, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_release_status(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn repository_inspection() -> Result<RepositoryInspectionSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_repository_inspection(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn repository_project_detail(repository_id: i64) -> Result<RepositoryInspectionEntry, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_repository_project_detail(&config, repository_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn build_history() -> Result<Vec<BuildHistoryRecord>, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_build_history(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn artifact_inspection() -> Result<Vec<ArtifactInspectionRecord>, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_artifact_inspection(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn build_execution_report(build_run_id: i64) -> Result<BuildExecutionReportPayload, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_build_execution_report(&config, build_run_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn read_retained_log_archive_entry(
    build_run_id: i64,
    entry_path: String,
    max_bytes: Option<usize>,
) -> Result<RetainedLogArchiveEntryPreviewPayload, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_retained_log_archive_entry(
        &config,
        build_run_id,
        &entry_path,
        normalize_text_file_preview_max_bytes(max_bytes),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn purge_build_execution_retention(
    build_run_id: i64,
) -> Result<BuildExecutionRetentionPurgeReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    purge_build_execution_retention_files(&config, build_run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_release_process_outputs(
    release_run_id: i64,
) -> Result<ReleaseProcessOutputsDeleteReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    delete_release_process_outputs_files(&config, release_run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn dispatch_on_demand_release_process(
    app_handle: AppHandle,
    input: OnDemandReleaseProcessCommandInput,
) -> Result<ReleaseRunRecord, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    let record = request_on_demand_release_process(&config, input)
        .map_err(|error| error.to_string())?;

    launch_runtime_process_handle(&app_handle).map_err(|error| error.to_string())?;
    Ok(record)
}

#[tauri::command]
fn rerun_release_process(
    app_handle: AppHandle,
    release_run_id: i64,
) -> Result<ReleaseRunRecord, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    let record = request_release_process_rerun(&config, release_run_id)
        .map_err(|error| error.to_string())?;

    launch_runtime_process_handle(&app_handle).map_err(|error| error.to_string())?;
    Ok(record)
}

#[tauri::command]
fn delete_build_log(build_run_id: i64) -> Result<BuildLogDeleteReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    delete_build_log_file(&config, build_run_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn auth_providers() -> Result<Vec<AuthProviderStatus>, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_auth_providers(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_repository_provider(
    input: RepositoryAccessAssessmentInput,
) -> Result<RepositoryProviderDetection, String> {
    detect_git_repository_provider(&input.repository_url)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn assess_repository_access(
    input: RepositoryAccessAssessmentInput,
) -> Result<RepositoryAccessAssessment, String> {
    assess_git_repository_access(&input.repository_url).map_err(|error| error.to_string())
}

#[tauri::command]
fn login_github_auth(force: Option<bool>) -> Result<AuthProviderStatus, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_github_auth_login(&config, force.unwrap_or(false))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn secret_settings() -> Result<SecretSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_secret_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_secret_credential(input: SaveSecretCredentialInput) -> Result<i64, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_secret_credential(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn connect_repository_auth(input: ConnectRepositoryAuthInput) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_auth_connect(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn reconnect_repository_auth(input: ReconnectRepositoryAuthInput) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_auth_reconnect(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn disconnect_repository_auth(
    input: DisconnectRepositoryAuthInput,
) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_auth_disconnect(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn sync_repository_auth_assessment(
    input: SyncRepositoryAuthAssessmentInput,
) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_auth_assessment(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_publish_target_secret_binding(
    input: UpdatePublishTargetSecretBindingInput,
) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_publish_target_secret_binding(&config, input)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn start_runtime(app_handle: AppHandle) -> Result<(), String> {
    launch_runtime_process_handle(&app_handle).map_err(|error| error.to_string())
}

#[tauri::command]
fn stop_runtime(app_handle: AppHandle) -> Result<(), String> {
    request_runtime_stop(&app_handle).map_err(|error| error.to_string())
}

#[tauri::command]
fn restart_runtime(app_handle: AppHandle) -> Result<(), String> {
    request_runtime_stop(&app_handle).map_err(|error| error.to_string())?;
    launch_runtime_process_handle(&app_handle).map_err(|error| error.to_string())
}

#[tauri::command]
fn set_runtime_automation_mode(
    mode: String,
) -> Result<RuntimeAutomationSnapshot, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    let storage = StorageLayout::from_directories(&config.directories);
    let normalized = mode.trim().to_ascii_lowercase();
    let parsed_mode = match normalized.as_str() {
        "active" => RuntimeAutomationMode::Active,
        "idle" => RuntimeAutomationMode::Idle,
        _ => {
            return Err(String::from(
                "runtime automation mode must be either 'active' or 'idle'",
            ));
        }
    };

    let snapshot = persist_runtime_automation_mode(&storage, parsed_mode)
        .map_err(|error| error.to_string())?;

    emit_runtime_event(
        &storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_AUTOMATION_MODE_CHANGED),
            severity: String::from("info"),
            origin: String::from("desktop-shell"),
            user_requested: true,
            repository_id: None,
            release_run_id: None,
            build_run_id: None,
            publish_run_id: None,
            summary: format!(
                "Automatic polling {} for the local host",
                if parsed_mode == RuntimeAutomationMode::Idle {
                    "paused"
                } else {
                    "resumed"
                }
            ),
            payload: serde_json::json!({
                "mode": parsed_mode.as_str(),
                "status": "updated",
            }),
        },
    )
    .map_err(|error| error.to_string())?;

    Ok(snapshot)
}

#[tauri::command]
fn request_repository_instant_check(
    app_handle: AppHandle,
    input: RepositoryInstantCheckInput,
) -> Result<(), String> {
    if input.repository_id <= 0 {
        return Err(String::from("repository_id must be a positive integer"));
    }

    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    let storage = StorageLayout::from_directories(&config.directories);
    enqueue_runtime_control_request(
        &storage,
        &RuntimeControlRequest::ForceRepositoryPoll {
            repository_id: input.repository_id,
        },
    )
    .map_err(|error| error.to_string())?;

    launch_runtime_process_handle(&app_handle).map_err(|error| error.to_string())
}

#[tauri::command]
fn unity_adapter_settings() -> Result<UnityAdapterSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_unity_adapter_settings(&config).map_err(|error| error.to_string())
}

fn launch_runtime_process<R: tauri::Runtime>(app: &tauri::App<R>) -> io::Result<()> {
    launch_runtime_process_handle(&app.handle())
}

fn launch_runtime_process_handle<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> io::Result<()> {
    let config = load_shell_runtime_config()?;
    if runtime_process_is_running(&config)? {
        return Ok(());
    }

    let plan = current_runtime_command_plan(RuntimeLaunchAction::Supervise)?;
    let mut command = plan.into_command();
    apply_runtime_command_environment(
        &mut command,
        &config.directories.data_dir,
        current_butler_sidecar_path().as_deref(),
    );
    let mut child = command.spawn()?;

    thread::sleep(Duration::from_millis(RUNTIME_STARTUP_PROBE_MILLIS));
    if let Some(status) = child.try_wait()? {
        return Err(io::Error::other(format!(
            "runtime process exited during shell startup with status {status}"
        )));
    }

    let state = app_handle.state::<RuntimeProcessState>();
    let mut guard = state
        .child
        .lock()
        .map_err(|_| io::Error::other("runtime process mutex is poisoned"))?;
    *guard = Some(child);
    Ok(())
}

fn stop_runtime_process<R: tauri::Runtime>(app_handle: &tauri::AppHandle<R>) {
    let _ = request_runtime_stop(app_handle);
}

fn request_runtime_stop<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> io::Result<()> {
    let config = load_shell_runtime_config()?;
    let state = app_handle.state::<RuntimeProcessState>();
    let child = match state.child.lock() {
        Ok(mut guard) => guard.take(),
        Err(_) => None,
    };

    if !runtime_process_is_running(&config)? {
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Ok(());
    }

    request_runtime_shutdown()?;

    if let Some(mut child) = child {
        for _ in 0..RUNTIME_SHUTDOWN_WAIT_POLLS {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => thread::sleep(Duration::from_millis(
                    RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS,
                )),
                Err(_) => break,
            }
        }

        if child.try_wait()?.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    for _ in 0..RUNTIME_SHUTDOWN_WAIT_POLLS {
        if !runtime_process_is_running(&config)? {
            return Ok(());
        }

        thread::sleep(Duration::from_millis(RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS));
    }

    Err(io::Error::other(
        "runtime did not stop within the configured grace period",
    ))
}

fn request_runtime_shutdown() -> io::Result<()> {
    let config = load_shell_runtime_config()?;
    let plan = current_runtime_command_plan(RuntimeLaunchAction::Shutdown)?;
    let mut command = plan.into_command();
    apply_runtime_command_environment(
        &mut command,
        &config.directories.data_dir,
        current_butler_sidecar_path().as_deref(),
    );
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "runtime shutdown command exited with status {status}"
        )))
    }
}

fn load_runtime_health_report(config: &RuntimeConfig) -> io::Result<RuntimeHealthReport> {
    let storage = StorageLayout::from_directories(&config.directories);
    read_health_report(&storage.health_report_path)
}

fn load_runtime_log_lines(
    config: &RuntimeConfig,
    line_limit: usize,
) -> io::Result<Vec<String>> {
    let storage = StorageLayout::from_directories(&config.directories);
    let content = fs::read_to_string(&storage.runtime_log_path)?;
    let mut lines = VecDeque::with_capacity(line_limit);

    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        if lines.len() == line_limit {
            lines.pop_front();
        }
        lines.push_back(line.to_owned());
    }

    Ok(lines.into_iter().collect())
}

fn normalize_runtime_log_line_limit(line_limit: Option<usize>) -> usize {
    line_limit
        .unwrap_or(DEFAULT_RUNTIME_LOG_LINE_LIMIT)
        .clamp(1, MAX_RUNTIME_LOG_LINE_LIMIT)
}

fn load_runtime_directory_settings(
    config: &RuntimeConfig,
) -> io::Result<RuntimeDirectorySettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);

    Ok(RuntimeDirectorySettings {
        data_dir: config.directories.data_dir.clone(),
        state_dir: config.directories.state_dir.clone(),
        logs_dir: config.directories.logs_dir.clone(),
        artifacts_dir: config.directories.artifacts_dir.clone(),
        runs_dir: config.directories.runs_dir.clone(),
        database_path: storage.database_path,
        health_report_path: storage.health_report_path,
        supervision_contract_path: storage.supervision_contract_path,
        supervisor_state_path: storage.supervisor_state_path,
        runtime_events_path: storage.runtime_events_path,
        runtime_events_cursor_path: storage.runtime_events_cursor_path,
        runtime_log_path: storage.runtime_log_path,
    })
}

fn resolve_localization_resource_root<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> io::Result<PathBuf> {
    if cfg!(debug_assertions) {
        return Ok(workspace_localization_resource_root());
    }

    app_handle
        .path()
        .resource_dir()
        .map(|path| path.join(LOCALIZATION_RESOURCE_DIR_NAME))
        .map_err(|error| io::Error::other(error.to_string()))
}

fn resolve_localization_settings_dir<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
) -> io::Result<PathBuf> {
    app_handle
        .path()
        .app_config_dir()
        .map_err(|error| io::Error::other(error.to_string()))
}

fn workspace_localization_resource_root() -> PathBuf {
    workspace_root()
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join(LOCALIZATION_RESOURCE_DIR_NAME)
}

fn localization_preferences_path(settings_dir: &Path) -> PathBuf {
    settings_dir.join(LOCALIZATION_SETTINGS_FILE_NAME)
}

fn load_localization_settings_from_paths(
    localization_root: &Path,
    settings_dir: &Path,
) -> io::Result<LocalizationSettings> {
    let (available_locales, mut warnings) =
        discover_localization_locale_settings(localization_root)?;
    let (persisted_preferences, persisted_warning) =
        read_persisted_localization_preferences(settings_dir)?;

    if let Some(persisted_warning) = persisted_warning {
        warnings.push(persisted_warning);
    }

    let primary_locale = resolve_configured_locale_code(
        persisted_preferences
            .as_ref()
            .map(|preferences| preferences.primary_locale.as_str()),
        default_primary_locale_code(&available_locales),
        &available_locales,
        "primary",
        &mut warnings,
    );
    let fallback_locale = resolve_configured_locale_code(
        persisted_preferences
            .as_ref()
            .map(|preferences| preferences.fallback_locale.as_str()),
        default_fallback_locale_code(&available_locales, &primary_locale),
        &available_locales,
        "fallback",
        &mut warnings,
    );

    Ok(LocalizationSettings {
        localization_root: localization_root.to_path_buf(),
        primary_locale,
        fallback_locale,
        available_locales,
        warnings,
    })
}

fn persist_localization_preferences_to_paths(
    localization_root: &Path,
    settings_dir: &Path,
    input: SaveLocalizationPreferencesInput,
) -> io::Result<LocalizationSettings> {
    let normalized_primary_locale = input.primary_locale.trim();
    let normalized_fallback_locale = input.fallback_locale.trim();
    let (available_locales, _) = discover_localization_locale_settings(localization_root)?;

    if available_locales.is_empty() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "no locale packs were discovered at {}",
                localization_root.display()
            ),
        ));
    }

    if normalized_primary_locale.is_empty() || normalized_fallback_locale.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "primary and fallback locale codes are required",
        ));
    }

    if !locale_code_exists(normalized_primary_locale, &available_locales) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "primary locale {:?} is not available in {}",
                normalized_primary_locale,
                localization_root.display()
            ),
        ));
    }

    if !locale_code_exists(normalized_fallback_locale, &available_locales) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "fallback locale {:?} is not available in {}",
                normalized_fallback_locale,
                localization_root.display()
            ),
        ));
    }

    fs::create_dir_all(settings_dir)?;
    fs::write(
        localization_preferences_path(settings_dir),
        serde_json::to_vec_pretty(&PersistedLocalizationPreferences {
            primary_locale: normalized_primary_locale.to_owned(),
            fallback_locale: normalized_fallback_locale.to_owned(),
        })
        .map_err(|error| io::Error::other(error.to_string()))?,
    )?;

    load_localization_settings_from_paths(localization_root, settings_dir)
}

fn discover_localization_locale_settings(
    localization_root: &Path,
) -> io::Result<(Vec<LocalizationLocaleSettings>, Vec<String>)> {
    let mut available_locales = Vec::new();
    let mut warnings = Vec::new();

    let entries = match fs::read_dir(localization_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            warnings.push(format!(
                "No locale packs were found at {}.",
                localization_root.display()
            ));
            return Ok((available_locales, warnings));
        }
        Err(error) => return Err(error),
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        {
            continue;
        }

        let Some(file_stem) = path.file_stem().and_then(|file_stem| file_stem.to_str()) else {
            warnings.push(format!(
                "Ignoring locale pack with a non-UTF8 file name at {}.",
                path.display()
            ));
            continue;
        };

        match load_localization_locale_setting(&path, file_stem) {
            Ok(locale_setting) => available_locales.push(locale_setting),
            Err(error) => warnings.push(format!(
                "Could not load locale pack {}: {}",
                path.display(),
                error
            )),
        }
    }

    available_locales.sort_by(|left, right| left.code.cmp(&right.code));
    Ok((available_locales, warnings))
}

fn load_localization_locale_setting(
    path: &Path,
    locale_code: &str,
) -> io::Result<LocalizationLocaleSettings> {
    let document = serde_json::from_slice::<LocalizationPackDocument>(&fs::read(path)?)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error.to_string()))?;
    let display_name = document.display_name.trim();
    if display_name.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "display_name is required",
        ));
    }

    let native_name = if document.native_name.trim().is_empty() {
        display_name.to_owned()
    } else {
        document.native_name.trim().to_owned()
    };

    Ok(LocalizationLocaleSettings {
        code: locale_code.trim().to_owned(),
        display_name: display_name.to_owned(),
        native_name,
        message_count: document.messages.len(),
        is_official: OFFICIAL_LOCALE_CODES.contains(&locale_code.trim()),
    })
}

fn read_persisted_localization_preferences(
    settings_dir: &Path,
) -> io::Result<(Option<PersistedLocalizationPreferences>, Option<String>)> {
    let settings_path = localization_preferences_path(settings_dir);
    let bytes = match fs::read(&settings_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok((None, None)),
        Err(error) => return Err(error),
    };

    match serde_json::from_slice::<PersistedLocalizationPreferences>(&bytes) {
        Ok(preferences) => Ok((Some(preferences), None)),
        Err(error) => Ok((
            None,
            Some(format!(
                "Saved localization settings at {} could not be parsed and were ignored: {}",
                settings_path.display(),
                error
            )),
        )),
    }
}

fn resolve_configured_locale_code(
    saved_locale_code: Option<&str>,
    preferred_default_locale_code: Option<&str>,
    available_locales: &[LocalizationLocaleSettings],
    setting_name: &str,
    warnings: &mut Vec<String>,
) -> String {
    if let Some(saved_locale_code) = saved_locale_code.map(str::trim) {
        if !saved_locale_code.is_empty()
            && locale_code_exists(saved_locale_code, available_locales)
        {
            return saved_locale_code.to_owned();
        }

        if !saved_locale_code.is_empty() {
            warnings.push(format!(
                "Saved {} locale {:?} is not available and was replaced.",
                setting_name,
                saved_locale_code
            ));
        }
    }

    if let Some(preferred_default_locale_code) = preferred_default_locale_code {
        return preferred_default_locale_code.to_owned();
    }

    String::from(DEFAULT_PRIMARY_LOCALE_CODE)
}

fn default_primary_locale_code(
    available_locales: &[LocalizationLocaleSettings],
) -> Option<&str> {
    if locale_code_exists(DEFAULT_PRIMARY_LOCALE_CODE, available_locales) {
        return Some(DEFAULT_PRIMARY_LOCALE_CODE);
    }

    available_locales.first().map(|locale| locale.code.as_str())
}

fn default_fallback_locale_code<'a>(
    available_locales: &'a [LocalizationLocaleSettings],
    primary_locale_code: &'a str,
) -> Option<&'a str> {
    if primary_locale_code != DEFAULT_FALLBACK_LOCALE_CODE
        && locale_code_exists(DEFAULT_FALLBACK_LOCALE_CODE, available_locales)
    {
        return Some(DEFAULT_FALLBACK_LOCALE_CODE);
    }

    if primary_locale_code != DEFAULT_PRIMARY_LOCALE_CODE
        && locale_code_exists(DEFAULT_PRIMARY_LOCALE_CODE, available_locales)
    {
        return Some(DEFAULT_PRIMARY_LOCALE_CODE);
    }

    available_locales
        .iter()
        .find(|locale| locale.code != primary_locale_code)
        .map(|locale| locale.code.as_str())
        .or(Some(primary_locale_code))
}

fn locale_code_exists(
    locale_code: &str,
    available_locales: &[LocalizationLocaleSettings],
) -> bool {
    available_locales
        .iter()
        .any(|locale| locale.code == locale_code)
}

fn load_runtime_lifecycle_settings(
    config: &RuntimeConfig,
) -> io::Result<RuntimeLifecycleSettings> {
    let storage = StorageLayout::from_directories(&config.directories);
    let startup_command = current_runtime_command_plan(RuntimeLaunchAction::Supervise)?;
    let shutdown_command = current_runtime_command_plan(RuntimeLaunchAction::Shutdown)?;
    let restart_policy = load_runtime_restart_policy(config, &storage)?;
    let supervisor_snapshot = match read_supervisor_snapshot(&storage.supervisor_state_path) {
        Ok(snapshot) => Some(snapshot),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };

    Ok(RuntimeLifecycleSettings {
        startup_command: startup_command.into(),
        shutdown_command: shutdown_command.into(),
        shell_launches_supervisor_on_startup: true,
        shell_requests_shutdown_on_exit: true,
        shell_force_kills_after_timeout: true,
        shell_relaunches_supervisor_on_restart: true,
        runtime_supervisor_owns_crash_recovery: true,
        shutdown_grace_period_millis: runtime_shutdown_grace_period_millis(),
        restart_policy,
        crash_recovery_status: runtime_crash_recovery_status(supervisor_snapshot.as_ref()),
        supervisor_snapshot,
    })
}

fn load_release_status(config: &RuntimeConfig) -> io::Result<AutomationSnapshot> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(AutomationSnapshot {
            generated_at: String::new(),
            queue_messages: Vec::new(),
            coordination_leases: Vec::new(),
            repositories: Vec::new(),
        });
    }

    LocalCoordinator::new(&storage).automation_snapshot()
}

fn load_repository_inspection(
    config: &RuntimeConfig,
) -> io::Result<RepositoryInspectionSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(RepositoryInspectionSettings {
            generated_at: String::new(),
            repositories: Vec::new(),
        });
    }

    let mut resources = load_repository_inspection_resources(config, &storage)?;

    let coordinator = LocalCoordinator::new(&storage);
    let repositories = coordinator
        .list_repository_projects()?
        .into_iter()
        .map(|repository| build_repository_inspection_entry(repository, &mut resources))
        .collect();

    Ok(RepositoryInspectionSettings {
        generated_at: resources.generated_at,
        repositories,
    })
}

fn load_repository_project_detail(
    config: &RuntimeConfig,
    repository_id: i64,
) -> io::Result<RepositoryInspectionEntry> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("repository id {repository_id} was not found"),
        ));
    }

    let mut resources = load_repository_inspection_resources(config, &storage)?;
    let repository = LocalCoordinator::new(&storage)
        .list_repository_projects()?
        .into_iter()
        .find(|repository| repository.id == repository_id)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("repository id {repository_id} was not found"),
            )
        })?;

    Ok(build_repository_inspection_entry(repository, &mut resources))
}

fn load_repository_inspection_resources(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> io::Result<RepositoryInspectionResources> {
    let release_status = load_release_status(config)?;
    let generated_at = release_status.generated_at.clone();
    let release_status_by_repository = release_status
        .repositories
        .into_iter()
        .map(|repository| (repository.repository_id, repository))
        .collect::<HashMap<_, _>>();
    let credential_by_id = load_repository_credential_references(storage)?;
    let build_targets_by_repository = load_repository_build_targets(storage)?;
    let publish_targets_by_repository =
        load_repository_publish_targets(storage, &credential_by_id)?;

    Ok(RepositoryInspectionResources {
        generated_at,
        release_status_by_repository,
        credential_by_id,
        build_targets_by_repository,
        publish_targets_by_repository,
    })
}

fn load_repository_credential_references(
    storage: &StorageLayout,
) -> io::Result<HashMap<i64, RepositoryCredentialReference>> {
    list_credential_records(storage)?
        .into_iter()
        .map(|credential| {
            let summary = summarize_credential_config(
                &credential.kind,
                &credential.config_json,
            );

            Ok((
                credential.id,
                RepositoryCredentialReference {
                    credential_id: credential.id,
                    name: credential.name,
                    kind: credential.kind,
                    config_status: summary.status,
                    config_message: summary.message,
                },
            ))
        })
        .collect()
}

fn load_repository_build_targets(
    storage: &StorageLayout,
) -> io::Result<HashMap<i64, Vec<UnityAdapterBuildTargetSettings>>> {
    let mut build_targets_by_repository =
        HashMap::<i64, Vec<UnityAdapterBuildTargetSettings>>::new();

    for target in list_build_target_runtime_settings(storage)? {
        let repository_id = target.repository_id;
        build_targets_by_repository
            .entry(repository_id)
            .or_default()
            .push(map_build_target_runner_settings(target));
    }

    Ok(build_targets_by_repository)
}

fn load_repository_publish_targets(
    storage: &StorageLayout,
    credential_by_id: &HashMap<i64, RepositoryCredentialReference>,
) -> io::Result<HashMap<i64, Vec<RepositoryPublishTargetInspection>>> {
    let mut publish_bindings_by_target =
        HashMap::<i64, Vec<RepositoryPublishBindingInspection>>::new();
    for binding in list_publish_target_binding_runtime_settings(storage)? {
        publish_bindings_by_target
            .entry(binding.publish_target_id)
            .or_default()
            .push(RepositoryPublishBindingInspection {
                build_target_id: binding.build_target_id,
                build_target_name: binding.build_target_name,
                enabled: binding.enabled,
                options_json: binding.options_json,
                consumption_behavior: binding.consumption_behavior,
            });
    }

    let mut publish_targets_by_repository =
        HashMap::<i64, Vec<RepositoryPublishTargetInspection>>::new();
    for target in list_publish_target_runtime_settings(storage)? {
        publish_targets_by_repository
            .entry(target.repository_id)
            .or_default()
            .push(RepositoryPublishTargetInspection {
                publish_target_id: target.id,
                name: target.name,
                kind: target.kind,
                enabled: target.enabled,
                config_json: target.config_json,
                credentials: clone_credential_reference(
                    credential_by_id,
                    target.credentials_id,
                ),
                bindings: publish_bindings_by_target
                    .remove(&target.id)
                    .unwrap_or_default(),
            });
    }

    Ok(publish_targets_by_repository)
}

fn build_repository_inspection_entry(
    repository: StoreRepositoryProjectRecord,
    resources: &mut RepositoryInspectionResources,
) -> RepositoryInspectionEntry {
    let release_status = resources.release_status_by_repository.get(&repository.id);

    RepositoryInspectionEntry {
        repository_id: repository.id,
        repository_name: repository.name,
        source_mode: repository.source_mode,
        workspace_strategy: repository.workspace_strategy,
        repo_url: repository
            .repo_url
            .clone()
            .or_else(|| repository.local_path.clone())
            .unwrap_or_default(),
        local_path: repository.local_path,
        engine_kind: repository.engine_kind,
        enabled: repository.enabled,
        polling_interval_seconds: repository.polling_interval_seconds,
        default_branch: repository.default_branch,
        artifacts_root_override: repository.artifacts_root_override,
        workspace_root_override: repository.workspace_root_override,
        last_seen_tag: repository.last_seen_tag,
        enabled_build_target_count: repository.enabled_build_target_count,
        credentials: clone_credential_reference(
            &resources.credential_by_id,
            repository.credentials_id,
        ),
        source_provider_id: repository.source_provider_id,
        source_instance_url: repository.source_instance_url,
        visibility_status: repository.visibility_status,
        auth_requirement_status: repository.auth_requirement_status,
        auth_binding_status: repository.auth_binding_status,
        auth_status_message: repository.auth_status_message,
        auth_last_verified_at: repository.auth_last_verified_at,
        build_targets: resources
            .build_targets_by_repository
            .remove(&repository.id)
            .unwrap_or_default(),
        publish_targets: resources
            .publish_targets_by_repository
            .remove(&repository.id)
            .unwrap_or_default(),
        pending_release_count: release_status
            .map(|status| status.pending_release_count)
            .unwrap_or(0),
        queued_build_runs: release_status
            .map(|status| status.queued_build_runs)
            .unwrap_or(0),
        running_build_runs: release_status
            .map(|status| status.running_build_runs)
            .unwrap_or(0),
        queued_publish_runs: release_status
            .map(|status| status.queued_publish_runs)
            .unwrap_or(0),
        running_publish_runs: release_status
            .map(|status| status.running_publish_runs)
            .unwrap_or(0),
        release_queue: release_status
            .map(|status| status.release_queue.clone())
            .unwrap_or_default(),
    }
}

fn load_build_history(config: &RuntimeConfig) -> io::Result<Vec<BuildHistoryRecord>> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(Vec::new());
    }

    list_build_history_records(&storage)
}

fn load_process_feed(
    config: &RuntimeConfig,
    input: ProcessFeedInput,
) -> io::Result<ProcessFeedPage> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    let (page, page_size) = input.normalized();
    if !storage.database_path.is_file() {
        return Ok(empty_process_feed_page(page, page_size));
    }

    list_process_feed_page(&storage, page, page_size)
}

fn request_release_process_rerun(
    config: &RuntimeConfig,
    release_run_id: i64,
) -> io::Result<ReleaseRunRecord> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("release run {release_run_id} was not found"),
        ));
    }

    let coordinator = LocalCoordinator::new(&storage);
    coordinator.get_release_run_record(release_run_id)?;

    let record = coordinator.dispatch_release_rebuild_by_id(
        release_run_id,
        DESKTOP_SHELL_RERUN_REQUESTED_VIA,
    )?;
    emit_shell_release_queued_event(&storage, &coordinator, &record)?;

    Ok(record)
}

fn request_on_demand_release_process(
    config: &RuntimeConfig,
    input: OnDemandReleaseProcessCommandInput,
) -> io::Result<ReleaseRunRecord> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("repository {} was not found", input.repository_id),
        ));
    }

    let coordinator = LocalCoordinator::new(&storage);
    let record = coordinator.dispatch_on_demand_release(
        normalize_on_demand_release_process_command_input(input),
    )?;
    emit_shell_release_queued_event(&storage, &coordinator, &record)?;

    Ok(record)
}

fn emit_shell_release_queued_event(
    storage: &StorageLayout,
    coordinator: &LocalCoordinator,
    record: &ReleaseRunRecord,
) -> io::Result<()> {
    let repository = coordinator.get_repository_checkout_record(record.repository_id)?;
    let source_metadata = decode_shell_release_source_metadata(record);
    let mode = shell_release_event_mode_label(
        record.trigger_source.as_str(),
        source_metadata.source_kind.as_deref(),
    );

    emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_RELEASE_QUEUED),
            severity: String::from("info"),
            origin: String::from("desktop-shell"),
            user_requested: true,
            repository_id: Some(record.repository_id),
            release_run_id: Some(record.id),
            build_run_id: None,
            publish_run_id: None,
            summary: format!(
                "{mode} release queued for {} {}",
                repository.name, record.git_tag
            ),
            payload: serde_json::json!({
                "repository_name": repository.name,
                "git_tag": record.git_tag,
                "git_commit": record.git_commit,
                "trigger_source": record.trigger_source,
                "source_kind": source_metadata.source_kind,
                "source_ref": source_metadata.source_ref,
                "local_path": source_metadata.local_path,
                "requested_via": source_metadata.requested_via,
                "status": "queued",
            }),
        },
    )?;

    Ok(())
}

fn decode_shell_release_source_metadata(record: &ReleaseRunRecord) -> ReleaseSourceMetadata {
    serde_json::from_str(record.source_metadata_json.trim()).unwrap_or_default()
}

fn shell_release_event_mode_label(
    trigger_source: &str,
    source_kind: Option<&str>,
) -> &'static str {
    if trigger_source.eq_ignore_ascii_case("poll") {
        return "Automatic";
    }

    match source_kind {
        Some("local_workspace") => "On-demand local",
        Some("managed_ref") => "On-demand ref",
        Some("managed_tag") => "On-demand tag",
        _ => "Manual",
    }
}

fn load_artifact_inspection(
    config: &RuntimeConfig,
) -> io::Result<Vec<ArtifactInspectionRecord>> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(Vec::new());
    }

    list_artifact_inspection_records(&storage)
}

fn build_execution_retained_dir(workspace_path: &Path) -> PathBuf {
    workspace_path.join(BUILD_EXECUTION_RETAINED_DIR_NAME)
}

fn empty_process_feed_page(page: u32, page_size: u32) -> ProcessFeedPage {
    ProcessFeedPage {
        generated_at: String::new(),
        page,
        page_size,
        total_items: 0,
        total_pages: 0,
        has_previous_page: false,
        has_next_page: false,
        items: Vec::new(),
    }
}

fn build_execution_report_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_REPORT_FILE_NAME)
}

fn build_execution_log_archive_path(workspace_path: &Path) -> PathBuf {
    build_execution_retained_dir(workspace_path).join(BUILD_EXECUTION_LOG_ARCHIVE_FILE_NAME)
}

fn load_build_execution_report(
    config: &RuntimeConfig,
    build_run_id: i64,
) -> io::Result<BuildExecutionReportPayload> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(BuildExecutionReportPayload {
            build_run_id,
            workspace_path: None,
            retained_dir_path: None,
            report_path: None,
            logs_archive_path: None,
            exists: false,
            logs_archive_exists: false,
            log_entries: Vec::new(),
            report: None,
        });
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let workspace_path = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let retained_dir_path = workspace_path.as_deref().map(build_execution_retained_dir);
    let report_path = workspace_path.as_deref().map(build_execution_report_path);
    let logs_archive_path = workspace_path.as_deref().map(build_execution_log_archive_path);
    let logs_archive_exists = logs_archive_path
        .as_ref()
        .map(|path| path.is_file())
        .unwrap_or(false);
    let log_entries = match logs_archive_path.as_deref() {
        Some(path) if path.is_file() => load_retained_log_archive_entries(path)?,
        _ => Vec::new(),
    };
    let report = match report_path.as_ref() {
        Some(path) if path.is_file() => Some(
            serde_json::from_slice(&fs::read(path)?)
                .map_err(io::Error::other)?,
        ),
        _ => None,
    };

    Ok(BuildExecutionReportPayload {
        build_run_id,
        workspace_path,
        retained_dir_path,
        report_path,
        logs_archive_path,
        exists: report.is_some(),
        logs_archive_exists,
        log_entries,
        report,
    })
}

fn load_retained_log_archive_entries(
    archive_path: &Path,
) -> io::Result<Vec<RetainedLogArchiveEntry>> {
    let archive_path = require_existing_regular_file(archive_path)?;
    let archive_file = fs::File::open(&archive_path)?;
    let mut archive = ZipArchive::new(archive_file).map_err(io::Error::other)?;
    let mut entries = Vec::new();

    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(io::Error::other)?;
        if entry.is_dir() {
            continue;
        }

        let entry_path = entry.name().to_owned();
        entries.push(RetainedLogArchiveEntry {
            entry_name: retained_log_archive_entry_name(&entry_path),
            entry_path,
            size_bytes: entry.size(),
            compressed_size_bytes: entry.compressed_size(),
        });
    }

    Ok(entries)
}

pub(crate) fn load_retained_log_archive_entry(
    config: &RuntimeConfig,
    build_run_id: i64,
    entry_path: &str,
    max_bytes: usize,
) -> io::Result<RetainedLogArchiveEntryPreviewPayload> {
    let normalized_entry_path = entry_path.trim();
    if normalized_entry_path.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "retained log archive entry path must not be empty",
        ));
    }

    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            "runtime database is unavailable for retained log lookup",
        ));
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let workspace_path = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                format!("build run {} does not have a retained workspace path", build_run_id),
            )
        })?;
    let archive_path = require_existing_regular_file(&build_execution_log_archive_path(&workspace_path))?;
    let archive_file = fs::File::open(&archive_path)?;
    let mut archive = ZipArchive::new(archive_file).map_err(io::Error::other)?;
    let mut entry = archive.by_name(normalized_entry_path).map_err(|_| {
        io::Error::new(
            ErrorKind::NotFound,
            format!(
                "retained log entry '{}' was not found in archive '{}'",
                normalized_entry_path,
                archive_path.display(),
            ),
        )
    })?;

    if entry.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("retained log entry '{}' is a directory", normalized_entry_path),
        ));
    }

    let size_bytes = entry.size();
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents)?;
    let truncated = contents.len() > max_bytes;
    let preview = if truncated {
        &contents[contents.len() - max_bytes..]
    } else {
        &contents
    };

    Ok(RetainedLogArchiveEntryPreviewPayload {
        archive_path,
        entry_path: normalized_entry_path.to_owned(),
        exists: true,
        size_bytes,
        truncated,
        content: String::from_utf8_lossy(preview).into_owned(),
    })
}

fn purge_build_execution_retention_files(
    config: &RuntimeConfig,
    build_run_id: i64,
) -> io::Result<BuildExecutionRetentionPurgeReport> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(BuildExecutionRetentionPurgeReport {
            build_run_id,
            workspace_path: None,
            retained_dir_path: None,
            removed_paths: Vec::new(),
            workspace_removed: false,
        });
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let workspace_path = build_run
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let retained_dir_path = workspace_path.as_deref().map(build_execution_retained_dir);
    let mut removed_paths = Vec::new();

    if let Some(retained_dir_path) = retained_dir_path.as_ref() {
        if retained_dir_path.exists() {
            fs::remove_dir_all(retained_dir_path)?;
            removed_paths.push(retained_dir_path.clone());
        }
    }

    let mut workspace_removed = false;
    if let Some(workspace_path) = workspace_path.as_ref() {
        if workspace_path.is_dir() {
            let is_empty = fs::read_dir(workspace_path)?.next().is_none();
            if is_empty {
                fs::remove_dir(workspace_path)?;
                removed_paths.push(workspace_path.clone());
                workspace_removed = true;
            }
        }
    }

    Ok(BuildExecutionRetentionPurgeReport {
        build_run_id,
        workspace_path,
        retained_dir_path,
        removed_paths,
        workspace_removed,
    })
}

fn delete_release_process_outputs_files(
    config: &RuntimeConfig,
    release_run_id: i64,
) -> io::Result<ReleaseProcessOutputsDeleteReport> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(ReleaseProcessOutputsDeleteReport {
            release_run_id,
            artifact_root_path: None,
            removed_paths: Vec::new(),
            missing_paths: Vec::new(),
        });
    }

    let artifact_root_path = list_build_history_records(&storage)?
        .into_iter()
        .find(|record| record.release_run_id == release_run_id)
        .and_then(|record| {
            record
                .artifact_root_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        });
    let mut removed_paths = Vec::new();
    let mut missing_paths = Vec::new();

    if let Some(path) = artifact_root_path.as_ref() {
        remove_directory_path(path, &mut removed_paths, &mut missing_paths)?;
    }

    Ok(ReleaseProcessOutputsDeleteReport {
        release_run_id,
        artifact_root_path,
        removed_paths,
        missing_paths,
    })
}

fn delete_build_log_file(
    config: &RuntimeConfig,
    build_run_id: i64,
) -> io::Result<BuildLogDeleteReport> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(BuildLogDeleteReport {
            build_run_id,
            log_path: None,
            removed_paths: Vec::new(),
            missing_paths: Vec::new(),
            parent_removed: false,
        });
    }

    let build_run = LocalCoordinator::new(&storage).get_build_run_record(build_run_id)?;
    let log_path = build_run
        .log_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let mut removed_paths = Vec::new();
    let mut missing_paths = Vec::new();
    let mut parent_removed = false;

    if let Some(path) = log_path.as_ref() {
        remove_file_path(path, &mut removed_paths, &mut missing_paths)?;

        if missing_paths.is_empty() {
            if let Some(parent) = path.parent() {
                if parent.is_dir() && fs::read_dir(parent)?.next().is_none() {
                    fs::remove_dir(parent)?;
                    removed_paths.push(parent.to_path_buf());
                    parent_removed = true;
                }
            }
        }
    }

    Ok(BuildLogDeleteReport {
        build_run_id,
        log_path,
        removed_paths,
        missing_paths,
        parent_removed,
    })
}

fn normalize_text_file_preview_max_bytes(max_bytes: Option<usize>) -> usize {
    match max_bytes {
        Some(value) if value > 0 => value.min(MAX_TEXT_FILE_PREVIEW_MAX_BYTES),
        _ => DEFAULT_TEXT_FILE_PREVIEW_MAX_BYTES,
    }
}

fn retained_log_archive_entry_name(entry_path: &str) -> String {
    entry_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(entry_path)
        .to_owned()
}

fn load_host_text_file(path: &Path, max_bytes: usize) -> io::Result<HostTextFilePayload> {
    let normalized_path = require_existing_regular_file(path)?;
    let contents = fs::read(&normalized_path)?;
    let size_bytes = contents.len() as u64;
    let truncated = contents.len() > max_bytes;
    let preview = if truncated {
        &contents[contents.len() - max_bytes..]
    } else {
        &contents
    };

    Ok(HostTextFilePayload {
        path: normalized_path,
        exists: true,
        size_bytes,
        truncated,
        content: String::from_utf8_lossy(preview).into_owned(),
    })
}

fn open_path_in_host(path: &Path) -> io::Result<()> {
    let normalized_path = require_existing_path(path)?;

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&normalized_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&normalized_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&normalized_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "host path opening is not supported on this platform",
    ))
}

fn open_url_in_host(url: &str) -> io::Result<()> {
    let normalized_url = normalize_external_url(url)?;

    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(normalized_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(normalized_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(normalized_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "external URL opening is not supported on this platform",
    ))
}

fn normalize_external_url(url: &str) -> io::Result<&str> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "external URL must not be empty",
        ));
    }

    if trimmed.chars().any(char::is_whitespace)
        || trimmed.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "external URL must not contain whitespace or control characters",
        ));
    }

    let Some((scheme, remainder)) = trimmed.split_once("://") else {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "external URL must include an http or https scheme",
        ));
    };

    if remainder.is_empty()
        || (!scheme.eq_ignore_ascii_case("http")
            && !scheme.eq_ignore_ascii_case("https"))
    {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "external URL must use http or https",
        ));
    }

    Ok(trimmed)
}

fn require_existing_path(path: &Path) -> io::Result<PathBuf> {
    let trimmed = path.to_string_lossy().trim().to_owned();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "host path must not be empty",
        ));
    }

    let normalized_path = PathBuf::from(trimmed);
    if !normalized_path.exists() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!("host path '{}' was not found", normalized_path.display()),
        ));
    }

    Ok(normalized_path)
}

fn require_existing_regular_file(path: &Path) -> io::Result<PathBuf> {
    let normalized_path = require_existing_path(path)?;
    if !normalized_path.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "host text file '{}' is not a regular file",
                normalized_path.display()
            ),
        ));
    }

    Ok(normalized_path)
}

fn remove_directory_path(
    path: &Path,
    removed_paths: &mut Vec<PathBuf>,
    missing_paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if !path.exists() {
        missing_paths.push(path.to_path_buf());
        return Ok(());
    }

    if !path.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("expected directory path '{}', found non-directory", path.display()),
        ));
    }

    fs::remove_dir_all(path)?;
    removed_paths.push(path.to_path_buf());
    Ok(())
}

fn remove_file_path(
    path: &Path,
    removed_paths: &mut Vec<PathBuf>,
    missing_paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    if !path.exists() {
        missing_paths.push(path.to_path_buf());
        return Ok(());
    }

    if !path.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("expected file path '{}', found non-file", path.display()),
        ));
    }

    fs::remove_file(path)?;
    removed_paths.push(path.to_path_buf());
    Ok(())
}

fn clone_credential_reference(
    credential_by_id: &HashMap<i64, RepositoryCredentialReference>,
    credential_id: Option<i64>,
) -> Option<RepositoryCredentialReference> {
    credential_id.and_then(|id| credential_by_id.get(&id).cloned())
}

fn load_auth_providers(config: &RuntimeConfig) -> io::Result<Vec<AuthProviderStatus>> {
    Ok(vec![load_github_auth_provider_status(config)?])
}

fn load_github_auth_provider_status(
    config: &RuntimeConfig,
) -> io::Result<AuthProviderStatus> {
    if !git_credential_manager_available() {
        return Ok(build_auth_provider_status(
            AUTH_PROVIDER_STATUS_UNAVAILABLE,
            String::from(
                "Git Credential Manager was not found on PATH, so GitHub login cannot start.",
            ),
            None,
            0,
        ));
    }

    let storage = writable_secret_storage(config)?;
    let known_accounts = match load_known_github_accounts() {
        Ok(accounts) => accounts,
        Err(error) => {
            let credential = resolve_github_auth_credential(&storage)?;
            let bound_repository_count = credential
                .as_ref()
                .map(|record| count_repository_bindings(&storage, record.id))
                .transpose()?
                .unwrap_or(0);

            return Ok(build_auth_provider_status(
                AUTH_PROVIDER_STATUS_UNAVAILABLE,
                format!(
                    "Git Credential Manager is installed but GitHub account discovery failed: {error}"
                ),
                credential.as_ref(),
                bound_repository_count,
            ));
        }
    };

    if known_accounts.is_empty() {
        let credential = resolve_github_auth_credential(&storage)?;
        let bound_repository_count = credential
            .as_ref()
            .map(|record| count_repository_bindings(&storage, record.id))
            .transpose()?
            .unwrap_or(0);

        return Ok(build_auth_provider_status(
            AUTH_PROVIDER_STATUS_DISCONNECTED,
            String::from(
                "No GitHub login is connected yet. Start the browser flow to authorize GitHub for repository operations.",
            ),
            credential.as_ref(),
            bound_repository_count,
        ));
    }

    let credential = ensure_github_auth_credential(&storage, &known_accounts)?;
    let bound_repository_count = count_repository_bindings(&storage, credential.id)?;

    Ok(build_auth_provider_status(
        AUTH_PROVIDER_STATUS_CONNECTED,
        format!(
            "Git Credential Manager has an active GitHub login. {bound_repository_count} repository project(s) currently connect to it explicitly."
        ),
        Some(&credential),
        bound_repository_count,
    ))
}

fn persist_github_auth_login(
    config: &RuntimeConfig,
    force: bool,
) -> io::Result<AuthProviderStatus> {
    ensure_git_credential_manager_available()?;
    run_github_browser_login_command(force)?;

    let storage = writable_secret_storage(config)?;

    finalize_github_auth_login(&storage)
}

fn finalize_github_auth_login(storage: &StorageLayout) -> io::Result<AuthProviderStatus> {
    let known_accounts = load_known_github_accounts()?;
    finalize_github_auth_login_with_known_accounts(storage, &known_accounts)
}

fn finalize_github_auth_login_with_known_accounts(
    storage: &StorageLayout,
    known_accounts: &[String],
) -> io::Result<AuthProviderStatus> {
    let credential = ensure_github_auth_credential(storage, known_accounts)?;
    let bound_repository_count = count_repository_bindings(storage, credential.id)?;

    Ok(build_auth_provider_status(
        AUTH_PROVIDER_STATUS_CONNECTED,
        format!(
            "GitHub login connected through Git Credential Manager. {bound_repository_count} repository project(s) currently connect to it explicitly."
        ),
        Some(&credential),
        bound_repository_count,
    ))
}

fn build_auth_provider_status(
    status: &str,
    status_message: String,
    credential: Option<&CredentialRecord>,
    bound_repository_count: usize,
) -> AuthProviderStatus {
    AuthProviderStatus {
        provider_id: String::from(GITHUB_AUTH_PROVIDER_ID),
        label: String::from(GITHUB_AUTH_PROVIDER_LABEL),
        status: String::from(status),
        status_message,
        instance_url: String::from(GITHUB_AUTH_INSTANCE_URL),
        credential_id: credential.map(|record| record.id),
        credential_name: credential.map(|record| record.name.clone()),
        credential_created_at: credential.map(|record| record.created_at.clone()),
        credential_updated_at: credential.map(|record| record.updated_at.clone()),
        bound_repository_count,
    }
}

fn ensure_git_credential_manager_available() -> io::Result<()> {
    if git_credential_manager_available() {
        return Ok(());
    }

    Err(io::Error::new(
        ErrorKind::NotFound,
        "Git Credential Manager was not found on PATH",
    ))
}

fn git_credential_manager_available() -> bool {
    run_git_credential_manager_command(["--version"]).is_ok()
}

fn github_browser_login_command_args(force: bool) -> Vec<&'static str> {
    let mut args = vec!["github", "login", "--browser"];
    if force {
        args.push("--force");
    }
    args.push("--url");
    args.push(GITHUB_AUTH_INSTANCE_URL);

    args
}

fn run_github_browser_login_command(force: bool) -> io::Result<()> {
    let _ = run_git_credential_manager_command(github_browser_login_command_args(force))?;

    Ok(())
}

fn load_known_github_accounts() -> io::Result<Vec<String>> {
    let output = run_git_credential_manager_command([
        "github",
        "list",
        "--url",
        GITHUB_AUTH_INSTANCE_URL,
    ])?;

    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn run_git_credential_manager_command<I, S>(args: I) -> io::Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = args
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect::<Vec<_>>();
    let preview = args.join(" ");
    let mut command = Command::new("git");
    command.arg("credential-manager");
    command.args(args.iter().map(String::as_str));
    command.stdin(Stdio::null());

    let output = command.output().map_err(|error| {
        io::Error::other(format!(
            "spawn git credential-manager {preview}: {error}"
        ))
    })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    let exit_detail = match output.status.code() {
        Some(code) => format!("exit code {code}"),
        None => String::from("termination by signal"),
    };
    let mut details = format!(
        "git credential-manager {preview} failed with {exit_detail}"
    );
    if !stderr.is_empty() {
        details.push_str("; stderr: ");
        details.push_str(stderr);
    }
    if !stdout.is_empty() {
        details.push_str("; stdout: ");
        details.push_str(stdout);
    }

    Err(io::Error::other(details))
}

fn resolve_github_auth_credential(
    storage: &StorageLayout,
) -> io::Result<Option<CredentialRecord>> {
    Ok(list_credential_records(storage)?
        .into_iter()
        .find(|credential| credential.kind == KIND_GIT_HTTP_GITHUB_HOST_LOGIN))
}

fn ensure_github_auth_credential(
    storage: &StorageLayout,
    known_accounts: &[String],
) -> io::Result<CredentialRecord> {
    let existing = resolve_github_auth_credential(storage)?;
    let selected_login = select_github_auth_login(
        existing
            .as_ref()
            .and_then(|credential| github_auth_credential_login(&credential.config_json)),
        known_accounts,
    );

    LocalCoordinator::new(storage).upsert_credential_record(
        UpsertCredentialRecordInput {
            credential_id: existing.as_ref().map(|credential| credential.id),
            name: String::from(GITHUB_AUTH_CREDENTIAL_NAME),
            kind: String::from(KIND_GIT_HTTP_GITHUB_HOST_LOGIN),
            config_json: github_auth_credential_config_json(selected_login.as_deref()),
        },
    )
}

fn github_auth_credential_config_json(login: Option<&str>) -> String {
    let mut config = serde_json::json!({
        "provider": GITHUB_AUTH_PROVIDER_ID,
        "instance_url": GITHUB_AUTH_INSTANCE_URL,
        "credential_helper": GITHUB_AUTH_CREDENTIAL_HELPER,
        "auth_mode": GITHUB_AUTH_MODE_BROWSER,
    });
    if let Some(login) = normalize_optional_auth_login(login) {
        config["login"] = serde_json::Value::String(login);
    }

    config.to_string()
}

fn github_auth_credential_login(config_json: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct GithubAuthCredentialConfig {
        #[serde(default)]
        login: Option<String>,
    }

    serde_json::from_str::<GithubAuthCredentialConfig>(config_json)
        .ok()
        .and_then(|config| normalize_optional_auth_login(config.login.as_deref()))
}

fn select_github_auth_login(
    existing_login: Option<String>,
    known_accounts: &[String],
) -> Option<String> {
    let normalized_accounts = normalize_known_github_accounts(known_accounts);
    if normalized_accounts.is_empty() {
        return existing_login;
    }

    if let Some(existing_login) =
        existing_login.and_then(|login| normalize_optional_auth_login(Some(&login)))
    {
        if normalized_accounts.iter().any(|account| account == &existing_login) {
            return Some(existing_login);
        }
    }

    preferred_known_github_login(&normalized_accounts)
        .or_else(|| normalized_accounts.first().cloned())
}

fn normalize_known_github_accounts(known_accounts: &[String]) -> Vec<String> {
    let mut normalized_accounts = Vec::new();
    let mut seen_accounts = HashSet::new();

    for account in known_accounts {
        let Some(account) = normalize_optional_auth_login(Some(account.as_str())) else {
            continue;
        };
        if seen_accounts.insert(account.clone()) {
            normalized_accounts.push(account);
        }
    }

    normalized_accounts
}

fn preferred_known_github_login(known_accounts: &[String]) -> Option<String> {
    known_accounts
        .iter()
        .filter(|account| !github_auth_login_is_placeholder(account))
        .filter(|account| !account.chars().all(|character| character.is_ascii_digit()))
    .next()
        .cloned()
}

fn github_auth_login_is_placeholder(login: &str) -> bool {
    matches!(
        login.trim().to_ascii_lowercase().as_str(),
        "x-access-token" | "x-oauth-basic"
    )
}

fn normalize_optional_auth_login(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn count_repository_bindings(
    storage: &StorageLayout,
    credential_id: i64,
) -> io::Result<usize> {
    Ok(LocalCoordinator::new(storage)
        .list_polling_repositories()?
        .into_iter()
        .filter(|repository| repository.credentials_id == Some(credential_id))
        .count())
}

fn writable_secret_storage(config: &RuntimeConfig) -> io::Result<StorageLayout> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        initialize_database(&storage)?;
    }

    Ok(storage)
}

fn load_secret_settings(config: &RuntimeConfig) -> io::Result<SecretSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    if !storage.database_path.is_file() {
        return Ok(SecretSettings {
            storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
            supported_credential_kinds: supported_credential_kinds(),
            warnings: secret_settings_warnings(),
            credentials: Vec::new(),
            repository_bindings: Vec::new(),
            publish_target_bindings: Vec::new(),
        });
    }

    let credentials = list_credential_records(&storage)?;
    let repository_bindings = LocalCoordinator::new(&storage)
        .list_polling_repositories()?
        .into_iter()
        .map(|repository| RepositorySecretBindingSetting {
            repository_id: repository.id,
            repository_name: repository.name,
            credentials_id: repository.credentials_id,
            enabled: repository.enabled,
        })
        .collect::<Vec<_>>();
    let publish_target_bindings = list_publish_target_runtime_settings(&storage)?
        .into_iter()
        .map(|target| PublishTargetSecretBindingSetting {
            publish_target_id: target.id,
            repository_id: target.repository_id,
            repository_name: target.repository_name,
            publish_target_name: target.name,
            publish_target_kind: target.kind,
            credentials_id: target.credentials_id,
            enabled: target.enabled,
        })
        .collect::<Vec<_>>();
    let credential_settings = credentials
        .into_iter()
        .map(|credential| {
            let bindings = credential_binding_references(
                credential.id,
                &repository_bindings,
                &publish_target_bindings,
            );

            SecretCredentialSetting {
                credential_id: credential.id,
                name: credential.name,
                kind: credential.kind.clone(),
                created_at: credential.created_at,
                updated_at: credential.updated_at,
                storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
                config_summary: summarize_credential_config(
                    &credential.kind,
                    &credential.config_json,
                ),
                bindings,
            }
        })
        .collect();

    Ok(SecretSettings {
        storage_model: String::from(SECRET_STORAGE_MODEL_INLINE_SQLITE),
        supported_credential_kinds: supported_credential_kinds(),
        warnings: secret_settings_warnings(),
        credentials: credential_settings,
        repository_bindings,
        publish_target_bindings,
    })
}

fn persist_secret_credential(
    config: &RuntimeConfig,
    input: SaveSecretCredentialInput,
) -> io::Result<i64> {
    if !credential_kind_supported(&input.kind) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "credentials kind {:?} is not translated by the current runtime",
                input.kind
            ),
        ));
    }

    let storage = writable_secret_storage(config)?;
    let credential = LocalCoordinator::new(&storage).upsert_credential_record(
        UpsertCredentialRecordInput {
            credential_id: input.credential_id,
            name: input.name,
            kind: input.kind,
            config_json: input.config_json,
        },
    )?;

    Ok(credential.id)
}

fn persist_repository_auth_binding(
    config: &RuntimeConfig,
    repository_id: i64,
    credentials_id: Option<i64>,
) -> io::Result<()> {
    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).update_repository_credentials_binding(repository_id, credentials_id)
}

fn persist_repository_auth_connect(
    config: &RuntimeConfig,
    input: ConnectRepositoryAuthInput,
) -> io::Result<()> {
    if input.credentials_id <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository credentials_id must be a positive integer",
        ));
    }

    persist_repository_auth_binding(config, input.repository_id, Some(input.credentials_id))
}

fn persist_repository_auth_reconnect(
    config: &RuntimeConfig,
    input: ReconnectRepositoryAuthInput,
) -> io::Result<()> {
    if input.credentials_id <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository credentials_id must be a positive integer",
        ));
    }

    persist_repository_auth_binding(config, input.repository_id, Some(input.credentials_id))
}

fn persist_repository_auth_disconnect(
    config: &RuntimeConfig,
    input: DisconnectRepositoryAuthInput,
) -> io::Result<()> {
    persist_repository_auth_binding(config, input.repository_id, None)
}

fn persist_repository_auth_assessment(
    config: &RuntimeConfig,
    input: SyncRepositoryAuthAssessmentInput,
) -> io::Result<()> {
    if input.repository_id <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository id must be a positive integer",
        ));
    }

    let storage = writable_secret_storage(config)?;
    persist_repository_auth_state_snapshot(
        &storage,
        input.repository_id,
        &input.repository_access_assessment,
    )
}

fn persist_publish_target_secret_binding(
    config: &RuntimeConfig,
    input: UpdatePublishTargetSecretBindingInput,
) -> io::Result<()> {
    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).update_publish_target_credentials_binding(
        input.publish_target_id,
        input.credentials_id,
    )
}

fn persist_repository_project(
    config: &RuntimeConfig,
    input: CreateRepositoryProjectCommandInput,
) -> io::Result<CreatedRepositoryProjectRecord> {
    let normalized = normalize_create_repository_project_command_input(input)?;
    let storage = writable_secret_storage(config)?;
    let repository_access_assessment = normalized.repository_access_assessment.clone();

    let mut created = LocalCoordinator::new(&storage).create_repository_project(
        StoreCreateRepositoryProjectInput {
            name: normalized.name,
            engine_kind: normalized.engine_kind,
            source_mode: normalized.source_mode,
            repo_url: normalized.repository_url,
            local_path: normalized.local_path,
            credentials: None,
            default_branch: normalized.default_branch,
            artifacts_root_override: normalized.artifacts_root_override,
            workspace_root_override: normalized.workspace_root_override,
            polling_interval_seconds: normalized.polling_interval_seconds,
            enabled: true,
            build_targets: normalized
                .build_targets
                .into_iter()
                .map(|target| CreateRepositoryProjectBuildTargetInput {
                    name: target.name,
                    build_kind: String::from("player"),
                    runner_type: String::from(RunnerFamily::HostNative.label()),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: DEFAULT_BUILD_TARGET_TIMEOUT_SECONDS,
                    enabled: true,
                    contract_json: unity_contract_json(
                        &target.target_platform,
                        &target.build_method,
                    ),
                    runner_config_json: unity_runner_config_json(
                        &target.unity_executable_path,
                    ),
                })
                .collect(),
            publish_targets: normalized
                .publish_targets
                .into_iter()
                .map(|target| StoreCreateRepositoryProjectPublishTargetInput {
                    name: target.name,
                    kind: target.kind,
                    enabled: target.enabled,
                    config_json: target.config_json,
                    credentials_id: target.credentials_id,
                    bindings: target
                        .bindings
                        .into_iter()
                        .map(|binding| StoreCreateRepositoryProjectPublishBindingInput {
                            build_target_name: binding.build_target_name,
                            enabled: binding.enabled,
                            options_json: binding.options_json,
                        })
                        .collect(),
                })
                .collect(),
        },
    )?;

    if let Some(repository_credentials_id) = normalized.repository_credentials_id {
        LocalCoordinator::new(&storage).update_repository_credentials_binding(
            created.repository_id,
            Some(repository_credentials_id),
        )?;
        created.credentials_id = Some(repository_credentials_id);
    }

    if let Some(assessment) = repository_access_assessment.as_ref() {
        persist_repository_auth_state_snapshot(
            &storage,
            created.repository_id,
            assessment,
        )?;
    }

    Ok(created)
}

fn persist_repository_project_update(
    config: &RuntimeConfig,
    input: UpdateRepositoryProjectCommandInput,
) -> io::Result<()> {
    let normalized = normalize_update_repository_project_command_input(input)?;
    let storage = writable_secret_storage(config)?;
    let repository_access_assessment = normalized.repository_access_assessment.clone();

    LocalCoordinator::new(&storage).update_repository_project(
        StoreUpdateRepositoryProjectInput {
            repository_id: normalized.repository_id,
            name: normalized.name,
            engine_kind: normalized.engine_kind,
            source_mode: normalized.source_mode,
            repo_url: normalized.repository_url,
            local_path: normalized.local_path,
            default_branch: normalized.default_branch,
            artifacts_root_override: normalized.artifacts_root_override,
            workspace_root_override: normalized.workspace_root_override,
            polling_interval_seconds: normalized.polling_interval_seconds,
            enabled: normalized.enabled,
            build_targets: normalized
                .build_targets
                .into_iter()
                .map(|target| StoreUpdateRepositoryProjectBuildTargetInput {
                    build_target_id: target.build_target_id,
                    name: target.name,
                    build_kind: String::from("player"),
                    runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: DEFAULT_BUILD_TARGET_TIMEOUT_SECONDS,
                    enabled: true,
                    contract_json: unity_contract_json(
                        &target.target_platform,
                        &target.build_method,
                    ),
                    runner_config_json: unity_runner_config_json(
                        &target.unity_executable_path,
                    ),
                })
                .collect(),
            publish_targets: normalized
                .publish_targets
                .into_iter()
                .map(|target| StoreUpdateRepositoryProjectPublishTargetInput {
                    publish_target_id: target.publish_target_id,
                    name: target.name,
                    kind: target.kind,
                    enabled: target.enabled,
                    config_json: target.config_json,
                    credentials_id: target.credentials_id,
                    bindings: target
                        .bindings
                        .into_iter()
                        .map(|binding| StoreUpdateRepositoryProjectPublishBindingInput {
                            build_target_id: binding.build_target_id,
                            build_target_name: binding.build_target_name,
                            enabled: binding.enabled,
                            options_json: binding.options_json,
                        })
                        .collect(),
                })
                .collect(),
        },
    )?;

    if let Some(assessment) = repository_access_assessment.as_ref() {
        persist_repository_auth_state_snapshot(
            &storage,
            normalized.repository_id,
            assessment,
        )?;
    }

    Ok(())
}

fn persist_repository_project_removal(
    config: &RuntimeConfig,
    input: RemoveRepositoryProjectCommandInput,
) -> io::Result<RepositoryProjectDeleteReport> {
    if input.repository_id <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository_id must be a positive integer",
        ));
    }

    let storage = writable_secret_storage(config)?;
    let report = LocalCoordinator::new(&storage).remove_repository_project(
        StoreRemoveRepositoryProjectInput {
            repository_id: input.repository_id,
            strategy: input.strategy,
        },
    )?;

    let mut removed_paths = Vec::new();
    let mut missing_paths = Vec::new();
    let mut skipped_paths = Vec::new();
    if matches!(report.strategy, RemoveRepositoryProjectStrategy::Purge) {
        purge_repository_project_files(
            &report,
            &mut removed_paths,
            &mut missing_paths,
            &mut skipped_paths,
        )?;
    }

    Ok(RepositoryProjectDeleteReport {
        repository_id: report.repository_id,
        repository_name: report.repository_name,
        strategy: report.strategy,
        release_run_count: report.release_run_count,
        build_run_count: report.build_run_count,
        publish_run_count: report.publish_run_count,
        queue_message_count: report.queue_message_count,
        coordination_lease_count: report.coordination_lease_count,
        idempotency_key_count: report.idempotency_key_count,
        removed_paths,
        missing_paths,
        skipped_paths,
    })
}

fn purge_repository_project_files(
    report: &StoreRemoveRepositoryProjectReport,
    removed_paths: &mut Vec<PathBuf>,
    missing_paths: &mut Vec<PathBuf>,
    skipped_paths: &mut Vec<PathBuf>,
) -> io::Result<()> {
    let normalized = normalize_repository_project_removal_paths(
        &report.directory_paths,
        &report.file_paths,
        skipped_paths,
    );

    for path in normalized.directory_paths {
        remove_directory_path(&path, removed_paths, missing_paths)?;
    }

    for path in normalized.file_paths {
        remove_file_path(&path, removed_paths, missing_paths)?;
    }

    Ok(())
}

fn normalize_repository_project_removal_paths(
    directory_paths: &[String],
    file_paths: &[String],
    skipped_paths: &mut Vec<PathBuf>,
) -> NormalizedRepositoryProjectRemovalPaths {
    let mut directory_paths = normalize_repository_project_path_list(
        directory_paths,
        skipped_paths,
    );
    directory_paths.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });

    let mut normalized_directory_paths = Vec::new();
    for path in directory_paths {
        if normalized_directory_paths
            .iter()
            .any(|ancestor| path.starts_with(ancestor))
        {
            continue;
        }

        normalized_directory_paths.push(path);
    }

    let mut normalized_file_paths = normalize_repository_project_path_list(
        file_paths,
        skipped_paths,
    );
    normalized_file_paths.sort();
    normalized_file_paths.retain(|path| {
        !normalized_directory_paths
            .iter()
            .any(|ancestor| path.starts_with(ancestor))
    });

    NormalizedRepositoryProjectRemovalPaths {
        directory_paths: normalized_directory_paths,
        file_paths: normalized_file_paths,
    }
}

fn normalize_repository_project_path_list(
    paths: &[String],
    skipped_paths: &mut Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut seen_paths = HashSet::new();
    let mut normalized_paths = Vec::new();

    for raw_path in paths {
        let trimmed_path = raw_path.trim();
        if trimmed_path.is_empty() {
            continue;
        }

        let path = PathBuf::from(trimmed_path);
        if !path.is_absolute() {
            skipped_paths.push(path);
            continue;
        }

        let dedupe_key = path.to_string_lossy().to_string();
        if !seen_paths.insert(dedupe_key) {
            continue;
        }

        normalized_paths.push(path);
    }

    normalized_paths
}

fn persist_repository_auth_state_snapshot(
    storage: &StorageLayout,
    repository_id: i64,
    assessment: &RepositoryAccessAssessment,
) -> io::Result<()> {
    LocalCoordinator::new(storage).update_repository_auth_state(
        StoreUpdateRepositoryAuthStateInput {
            repository_id,
            source_provider_id: assessment.provider_id.clone(),
            source_instance_url: assessment.instance_url.clone(),
            visibility_status: assessment.visibility.clone(),
            auth_requirement_status: assessment.auth_requirement.clone(),
            supports_interactive_login: assessment.supports_interactive_login,
            auth_status_message: assessment.message.clone(),
        },
    )
}

fn normalize_create_repository_project_command_input(
    input: CreateRepositoryProjectCommandInput,
) -> io::Result<NormalizedCreateRepositoryProjectCommandInput> {
    let engine_kind = normalize_repository_project_engine_kind(&input.engine_kind)?;
    let name = require_shell_non_empty(&input.name, "repository project name")?;
    let source_mode = normalize_optional_shell_string(input.source_mode)
        .unwrap_or_else(|| String::from("managed_repository"));
    let repository_url = normalize_optional_shell_string(input.repository_url);
    let local_path = normalize_optional_shell_string(input.local_path);

    match source_mode.as_str() {
        "managed_repository" => {
            let repository_url = repository_url.as_deref().ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project URL must not be empty",
                )
            })?;

            if !(repository_url.starts_with("https://")
                || repository_url.starts_with("http://"))
            {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project URL must use http:// or https://",
                ));
            }
        }
        "local_workspace" => {
            let local_path = local_path.as_deref().ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "local workspace path must not be empty",
                )
            })?;

            if !PathBuf::from(local_path).is_absolute() {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "local workspace path must be an absolute path",
                ));
            }
        }
        _ => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "repository project source_mode {:?} is not supported",
                    source_mode
                ),
            ));
        }
    }
    if input.polling_interval_seconds < MIN_REPOSITORY_POLL_INTERVAL_SECONDS {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "repository polling interval must be at least {MIN_REPOSITORY_POLL_INTERVAL_SECONDS} seconds"
            ),
        ));
    }
    if input.build_targets.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository project must define at least one build target",
        ));
    }

    if normalize_optional_shell_string(input.personal_access_token).is_some() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository personal access token is no longer supported; use the GitHub login flow",
        ));
    }

    if let Some(repository_credentials_id) = input.repository_credentials_id {
        if repository_credentials_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "repository_credentials_id must be a positive integer when provided",
            ));
        }
    }

    let mut build_target_names = std::collections::HashSet::new();
    let mut build_targets = Vec::with_capacity(input.build_targets.len());
    for target in input.build_targets {
        let normalized = normalize_create_repository_project_build_target_command_input(target)?;
        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !build_target_names.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "repository build target names must be unique",
            ));
        }
        build_targets.push(normalized);
    }

    let mut publish_target_names = std::collections::HashSet::new();
    let mut publish_targets = Vec::with_capacity(input.publish_targets.len());
    for target in input.publish_targets {
        let normalized = normalize_create_repository_project_publish_target_command_input(target)?;
        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !publish_target_names.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "repository publish target names must be unique",
            ));
        }
        publish_targets.push(normalized);
    }

    Ok(NormalizedCreateRepositoryProjectCommandInput {
        name,
        engine_kind,
        source_mode,
        repository_url,
        local_path,
        repository_access_assessment: input.repository_access_assessment,
        repository_credentials_id: input.repository_credentials_id,
        default_branch: normalize_optional_shell_string(input.default_branch),
        artifacts_root_override: normalize_optional_shell_string(input.artifacts_root_override),
        workspace_root_override: normalize_optional_shell_string(input.workspace_root_override),
        polling_interval_seconds: input.polling_interval_seconds,
        build_targets,
        publish_targets,
    })
}

fn normalize_update_repository_project_command_input(
    input: UpdateRepositoryProjectCommandInput,
) -> io::Result<NormalizedUpdateRepositoryProjectCommandInput> {
    if input.repository_id <= 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository_id must be a positive integer",
        ));
    }

    let engine_kind = normalize_repository_project_engine_kind(&input.engine_kind)?;
    let name = require_shell_non_empty(&input.name, "repository project name")?;
    let source_mode = require_shell_non_empty(&input.source_mode, "repository project source_mode")?
        .to_ascii_lowercase();
    let repository_url = normalize_optional_shell_string(input.repository_url);
    let local_path = normalize_optional_shell_string(input.local_path);

    match source_mode.as_str() {
        "managed_repository" => {
            let repository_url = repository_url.as_deref().ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project URL must not be empty",
                )
            })?;
            if !(repository_url.starts_with("https://") || repository_url.starts_with("http://")) {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project URL must use http:// or https://",
                ));
            }
        }
        "local_workspace" => {
            let local_path = local_path.as_deref().ok_or_else(|| {
                io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project local workspace path must not be empty",
                )
            })?;
            if !PathBuf::from(local_path).is_absolute() {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "repository project local workspace path must be absolute",
                ));
            }
        }
        _ => {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "repository project source_mode {:?} is not supported",
                    source_mode
                ),
            ));
        }
    }

    if input.polling_interval_seconds < MIN_REPOSITORY_POLL_INTERVAL_SECONDS {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "repository polling interval must be at least {MIN_REPOSITORY_POLL_INTERVAL_SECONDS} seconds"
            ),
        ));
    }

    if input.build_targets.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository project must define at least one build target",
        ));
    }

    let mut build_target_ids = HashSet::new();
    let mut build_target_names = HashSet::new();
    let mut build_targets = Vec::with_capacity(input.build_targets.len());
    for target in input.build_targets {
        let normalized = normalize_update_repository_project_build_target_command_input(target)?;
        if let Some(build_target_id) = normalized.build_target_id {
            if !build_target_ids.insert(build_target_id) {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "repository build target {build_target_id} was provided more than once"
                    ),
                ));
            }
        }

        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !build_target_names.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "repository build target names must be unique",
            ));
        }

        build_targets.push(normalized);
    }

    let mut publish_target_ids = HashSet::new();
    let mut publish_target_names = HashSet::new();
    let mut publish_targets = Vec::with_capacity(input.publish_targets.len());
    for target in input.publish_targets {
        let normalized = normalize_update_repository_project_publish_target_command_input(target)?;
        if let Some(publish_target_id) = normalized.publish_target_id {
            if !publish_target_ids.insert(publish_target_id) {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "repository publish target {publish_target_id} was provided more than once"
                    ),
                ));
            }
        }

        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !publish_target_names.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "repository publish target names must be unique",
            ));
        }

        publish_targets.push(normalized);
    }

    Ok(NormalizedUpdateRepositoryProjectCommandInput {
        repository_id: input.repository_id,
        name,
        engine_kind,
        source_mode,
        repository_url,
        local_path,
        repository_access_assessment: input.repository_access_assessment,
        default_branch: normalize_optional_shell_string(input.default_branch),
        artifacts_root_override: normalize_optional_shell_string(input.artifacts_root_override),
        workspace_root_override: normalize_optional_shell_string(input.workspace_root_override),
        polling_interval_seconds: input.polling_interval_seconds,
        enabled: input.enabled,
        build_targets,
        publish_targets,
    })
}

fn normalize_create_repository_project_build_target_command_input(
    input: CreateRepositoryProjectBuildTargetCommandInput,
) -> io::Result<NormalizedCreateRepositoryProjectBuildTargetCommandInput> {
    let unity_contract = input.contract.unity.ok_or_else(|| {
        io::Error::new(
            ErrorKind::InvalidInput,
            "build target contract.unity is required while Unity is the only supported engine",
        )
    })?;
    let unity_executable_path =
        require_shell_non_empty(&input.unity_executable_path, "build target Unity executable path")?;
    let diagnostics = validate_unity_executable_path_diagnostics(&unity_executable_path);
    if diagnostics.status != "ready" {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            diagnostics.message,
        ));
    }

    Ok(NormalizedCreateRepositoryProjectBuildTargetCommandInput {
        name: require_shell_non_empty(&input.name, "build target name")?,
        target_platform: require_shell_non_empty(
            &unity_contract.target_platform,
            "build target contract.unity.target_platform",
        )?,
        build_method: require_shell_non_empty(
            &unity_contract.build_method,
            "build target contract.unity.build_method",
        )?,
        unity_executable_path,
    })
}

fn normalize_update_repository_project_build_target_command_input(
    input: UpdateRepositoryProjectBuildTargetCommandInput,
) -> io::Result<NormalizedUpdateRepositoryProjectBuildTargetCommandInput> {
    if let Some(build_target_id) = input.build_target_id {
        if build_target_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "build_target_id must be a positive integer",
            ));
        }
    }

    let normalized = normalize_create_repository_project_build_target_command_input(
        CreateRepositoryProjectBuildTargetCommandInput {
            name: input.name,
            contract: input.contract,
            unity_executable_path: input.unity_executable_path,
        },
    )?;

    Ok(NormalizedUpdateRepositoryProjectBuildTargetCommandInput {
        build_target_id: input.build_target_id,
        name: normalized.name,
        target_platform: normalized.target_platform,
        build_method: normalized.build_method,
        unity_executable_path: normalized.unity_executable_path,
    })
}

fn normalize_create_repository_project_publish_target_command_input(
    input: CreateRepositoryProjectPublishTargetCommandInput,
) -> io::Result<NormalizedCreateRepositoryProjectPublishTargetCommandInput> {
    if let Some(credentials_id) = input.credentials_id {
        if credentials_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish target credentials_id must be a positive integer when provided",
            ));
        }
    }

    let mut build_target_names = HashSet::new();
    let mut bindings = Vec::with_capacity(input.bindings.len());
    for binding in input.bindings {
        let normalized = normalize_create_repository_project_publish_binding_command_input(binding)?;
        let duplicate_key = normalized.build_target_name.to_ascii_lowercase();
        if !build_target_names.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish target bindings must not repeat the same build target",
            ));
        }
        bindings.push(normalized);
    }

    Ok(NormalizedCreateRepositoryProjectPublishTargetCommandInput {
        name: require_shell_non_empty(&input.name, "publish target name")?,
        kind: require_shell_non_empty(&input.kind, "publish target kind")?
            .to_ascii_lowercase(),
        enabled: input.enabled,
        config_json: require_shell_non_empty(&input.config_json, "publish target config_json")?,
        credentials_id: input.credentials_id,
        bindings,
    })
}

fn normalize_create_repository_project_publish_binding_command_input(
    input: CreateRepositoryProjectPublishBindingCommandInput,
) -> io::Result<NormalizedCreateRepositoryProjectPublishBindingCommandInput> {
    Ok(NormalizedCreateRepositoryProjectPublishBindingCommandInput {
        build_target_name: require_shell_non_empty(
            &input.build_target_name,
            "publish binding build_target_name",
        )?,
        enabled: input.enabled,
        options_json: require_shell_non_empty(&input.options_json, "publish binding options_json")?,
    })
}

fn normalize_update_repository_project_publish_target_command_input(
    input: UpdateRepositoryProjectPublishTargetCommandInput,
) -> io::Result<NormalizedUpdateRepositoryProjectPublishTargetCommandInput> {
    if let Some(publish_target_id) = input.publish_target_id {
        if publish_target_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish_target_id must be a positive integer when provided",
            ));
        }
    }

    if let Some(credentials_id) = input.credentials_id {
        if credentials_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish target credentials_id must be a positive integer when provided",
            ));
        }
    }

    let mut claimed_build_targets = HashSet::new();
    let mut bindings = Vec::with_capacity(input.bindings.len());
    for binding in input.bindings {
        let normalized = normalize_update_repository_project_publish_binding_command_input(binding)?;
        let duplicate_key = normalized
            .build_target_id
            .map(|build_target_id| format!("id:{build_target_id}"))
            .unwrap_or_else(|| normalized.build_target_name.to_ascii_lowercase());
        if !claimed_build_targets.insert(duplicate_key) {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish target bindings must not repeat the same build target",
            ));
        }
        bindings.push(normalized);
    }

    Ok(NormalizedUpdateRepositoryProjectPublishTargetCommandInput {
        publish_target_id: input.publish_target_id,
        name: require_shell_non_empty(&input.name, "publish target name")?,
        kind: require_shell_non_empty(&input.kind, "publish target kind")?
            .to_ascii_lowercase(),
        enabled: input.enabled,
        config_json: require_shell_non_empty(&input.config_json, "publish target config_json")?,
        credentials_id: input.credentials_id,
        bindings,
    })
}

fn normalize_update_repository_project_publish_binding_command_input(
    input: UpdateRepositoryProjectPublishBindingCommandInput,
) -> io::Result<NormalizedUpdateRepositoryProjectPublishBindingCommandInput> {
    if let Some(build_target_id) = input.build_target_id {
        if build_target_id <= 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "publish binding build_target_id must be a positive integer when provided",
            ));
        }
    }

    Ok(NormalizedUpdateRepositoryProjectPublishBindingCommandInput {
        build_target_id: input.build_target_id,
        build_target_name: require_shell_non_empty(
            &input.build_target_name,
            "publish binding build_target_name",
        )?,
        enabled: input.enabled,
        options_json: require_shell_non_empty(&input.options_json, "publish binding options_json")?,
    })
}

fn normalize_repository_project_engine_kind(engine_kind: &str) -> io::Result<String> {
    let normalized = require_shell_non_empty(engine_kind, "repository project engine kind")?
        .to_ascii_lowercase();
    if normalized != SUPPORTED_REPOSITORY_ENGINE_KIND_UNITY {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "repository engine {:?} is not supported yet; only \"unity\" is currently allowed",
                normalized,
            ),
        ));
    }

    Ok(normalized)
}

fn unity_contract_json(target_platform: &str, build_method: &str) -> String {
    serde_json::json!({
        "unity": {
            "targetPlatform": target_platform.trim(),
            "buildMethod": build_method.trim(),
        }
    })
    .to_string()
}

fn unity_runner_config_json(unity_executable_path: &str) -> String {
    serde_json::json!({
        "unity_executable_path": unity_executable_path.trim(),
    })
    .to_string()
}

fn validate_unity_executable_path_diagnostics(path: &str) -> HostNativeRunnerDiagnostics {
    diagnose_host_native_runner_config(
        &serde_json::json!({
            "unity_executable_path": path.trim(),
        })
        .to_string(),
    )
}

fn require_shell_non_empty(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("{label} must not be empty"),
        ));
    }

    Ok(trimmed.to_owned())
}

fn normalize_optional_shell_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn normalize_on_demand_release_process_command_input(
    input: OnDemandReleaseProcessCommandInput,
) -> StoreOnDemandReleaseDispatchInput {
    StoreOnDemandReleaseDispatchInput {
        repository_id: input.repository_id,
        release_version: normalize_optional_shell_string(input.release_version),
        version_source: input.version_source.trim().to_owned(),
        source_kind: input.source_kind.trim().to_owned(),
        source_ref: normalize_optional_shell_string(input.source_ref),
        local_path: normalize_optional_shell_string(input.local_path),
        requested_via: String::from(DESKTOP_SHELL_RERUN_REQUESTED_VIA),
        unity_executable_path_override: normalize_optional_shell_string(
            input.unity_executable_path_override,
        ),
    }
}

fn credential_binding_references(
    credential_id: i64,
    repository_bindings: &[RepositorySecretBindingSetting],
    publish_target_bindings: &[PublishTargetSecretBindingSetting],
) -> Vec<SecretBindingReference> {
    let mut bindings = Vec::new();

    for repository in repository_bindings {
        if repository.credentials_id != Some(credential_id) {
            continue;
        }

        bindings.push(SecretBindingReference {
            binding_kind: String::from("repository"),
            binding_id: repository.repository_id,
            binding_name: repository.repository_name.clone(),
            repository_id: repository.repository_id,
            repository_name: repository.repository_name.clone(),
            enabled: repository.enabled,
        });
    }

    for publish_target in publish_target_bindings {
        if publish_target.credentials_id != Some(credential_id) {
            continue;
        }

        bindings.push(SecretBindingReference {
            binding_kind: String::from("publish_target"),
            binding_id: publish_target.publish_target_id,
            binding_name: publish_target.publish_target_name.clone(),
            repository_id: publish_target.repository_id,
            repository_name: publish_target.repository_name.clone(),
            enabled: publish_target.enabled,
        });
    }

    bindings
}

fn summarize_credential_config(kind: &str, config_json: &str) -> CredentialConfigSummary {
    let expected_keys = expected_credential_keys(kind);
    let parsed = match serde_json::from_str::<serde_json::Value>(config_json.trim()) {
        Ok(value) => value,
        Err(error) => {
            return CredentialConfigSummary {
                status: String::from("invalid_config_json"),
                message: format!(
                    "stored credential config_json is not valid JSON: {error}"
                ),
                top_level_keys: Vec::new(),
                missing_required_keys: expected_keys,
            }
        }
    };
    let Some(object) = parsed.as_object() else {
        return CredentialConfigSummary {
            status: String::from("invalid_config_shape"),
            message: String::from(
                "stored credential config_json must decode to a JSON object",
            ),
            top_level_keys: Vec::new(),
            missing_required_keys: expected_keys,
        };
    };

    let mut top_level_keys = object.keys().cloned().collect::<Vec<_>>();
    top_level_keys.sort();
    let missing_required_keys = expected_keys
        .iter()
        .filter(|key| !object.contains_key(key.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if !credential_kind_supported(kind) {
        return CredentialConfigSummary {
            status: String::from("unsupported_kind"),
            message: format!(
                "credentials kind {:?} is not translated by the current runtime",
                kind
            ),
            top_level_keys,
            missing_required_keys,
        };
    }

    if !missing_required_keys.is_empty() {
        return CredentialConfigSummary {
            status: String::from("incomplete_config"),
            message: format!(
                "stored credential config_json is missing required keys: {}",
                missing_required_keys.join(", ")
            ),
            top_level_keys,
            missing_required_keys,
        };
    }

    CredentialConfigSummary {
        status: String::from("ready"),
        message: String::from(
            "stored credential config_json decodes successfully and required keys are present",
        ),
        top_level_keys,
        missing_required_keys,
    }
}

fn supported_credential_kinds() -> Vec<String> {
    vec![
        String::from(KIND_GIT_HTTP_BASIC),
        String::from(KIND_GIT_HTTP_BEARER),
        String::from(KIND_GIT_HTTP_GITHUB_HOST_LOGIN),
        String::from(KIND_ITCH_API_KEY),
    ]
}

fn credential_kind_supported(kind: &str) -> bool {
    matches!(
        kind.trim(),
        KIND_GIT_HTTP_BASIC
            | KIND_GIT_HTTP_BEARER
            | KIND_GIT_HTTP_GITHUB_HOST_LOGIN
            | KIND_ITCH_API_KEY
    )
}

fn expected_credential_keys(kind: &str) -> Vec<String> {
    match kind.trim() {
        KIND_GIT_HTTP_BASIC => vec![String::from("password"), String::from("username")],
        KIND_GIT_HTTP_BEARER => vec![String::from("token")],
        KIND_GIT_HTTP_GITHUB_HOST_LOGIN => vec![
            String::from("auth_mode"),
            String::from("credential_helper"),
            String::from("instance_url"),
            String::from("provider"),
        ],
        KIND_ITCH_API_KEY => vec![String::from("api_key")],
        _ => Vec::new(),
    }
}

fn secret_settings_warnings() -> Vec<String> {
    vec![
        String::from(
            "credentials.config_json may contain either inline secret material or host keyring references",
        ),
        String::from(
            "manifest sync resolves env and file sources before persistence, so SQLite may already contain materialized secret values",
        ),
        String::from(
            "GitHub login credentials created through the desktop shell delegate secret storage to Git Credential Manager and store only runtime metadata in SQLite",
        ),
    ]
}

fn load_unity_adapter_settings(config: &RuntimeConfig) -> io::Result<UnityAdapterSettings> {
    config.directories.ensure_exists()?;
    let storage = StorageLayout::from_directories(&config.directories);
    let build_targets = if storage.database_path.is_file() {
        list_build_target_runtime_settings(&storage)?
            .into_iter()
            .map(map_build_target_runner_settings)
            .collect()
    } else {
        Vec::new()
    };
    let capability_profile = load_cached_host_capability_profile(config.platform);
    let mut supported_runner_families = vec![String::from(RunnerFamily::HostNative.label())];
    if let Some(selected_runner_family) = capability_profile
        .runner_selection
        .selected_runner_family
        .clone()
    {
        if !supported_runner_families.contains(&selected_runner_family) {
            supported_runner_families.push(selected_runner_family);
        }
    }

    Ok(UnityAdapterSettings {
        platform: String::from(config.platform.as_str()),
        supported_runner_families,
        discovery_roots: default_unity_discovery_roots(config.platform),
        capability_profile,
        build_targets,
    })
}

fn load_cached_host_capability_profile(
    platform: HostPlatform,
) -> HostCapabilityProfile {
    let cache_key = platform.as_str();

    if let Ok(cache) = HOST_CAPABILITY_PROFILE_CACHE.lock() {
        if let Some(entry) = cache.get(cache_key) {
            if entry.cached_at.elapsed() <= HOST_CAPABILITY_PROFILE_CACHE_TTL {
                return entry.profile.clone();
            }
        }
    }

    let profile = inspect_host_capability_profile(platform);
    if let Ok(mut cache) = HOST_CAPABILITY_PROFILE_CACHE.lock() {
        cache.insert(
            cache_key,
            CachedHostCapabilityProfile {
                cached_at: Instant::now(),
                profile: profile.clone(),
            },
        );
    }

    profile
}

fn default_unity_discovery_roots(
    platform: HostPlatform,
) -> Vec<UnityDiscoveryRootSetting> {
    default_unity_discovery_root_paths(platform)
        .into_iter()
        .map(|path| UnityDiscoveryRootSetting {
            exists: path.is_dir(),
            path,
        })
        .collect()
}

fn map_build_target_runner_settings(
    target: runtime_store::BuildTargetRuntimeSettingsRecord,
) -> UnityAdapterBuildTargetSettings {
    let diagnostic = if target.runner_type == RunnerFamily::HostNative.label() {
        Some(diagnose_host_native_runner_config(&target.config_json))
    } else {
        None
    };
    let (diagnostic_status, diagnostic_message) = match diagnostic.as_ref() {
        Some(diagnostic) => (
            diagnostic.status.clone(),
            diagnostic.message.clone(),
        ),
        None => (
            String::from("unsupported_runner_type"),
            format!(
                "runner type {:?} is not supported by the Tauri runtime",
                target.runner_type
            ),
        ),
    };

    UnityAdapterBuildTargetSettings {
        build_target_id: target.id,
        repository_id: target.repository_id,
        repository_name: target.repository_name,
        target_name: target.name,
        unity_target_platform: target.unity_target_platform,
        runner_type: target.runner_type,
        unity_build_method: target.unity_build_method,
        enabled: target.enabled,
        diagnostic_status,
        diagnostic_message,
        host_native_diagnostics: diagnostic,
    }
}

impl From<RuntimeCommandPlan> for RuntimeLifecycleCommandSettings {
    fn from(plan: RuntimeCommandPlan) -> Self {
        Self {
            program: plan.program,
            args: plan.args,
            current_dir: plan.current_dir,
        }
    }
}

fn load_runtime_restart_policy(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> io::Result<RuntimeRestartPolicy> {
    match read_supervision_contract(&storage.supervision_contract_path) {
        Ok(contract) => Ok(contract.restart_policy),
        Err(error) if is_ignorable_runtime_state_error(&error) => {
            Ok(RuntimeRestartPolicy::from_settings(&config.supervision))
        }
        Err(error) => Err(error),
    }
}

fn load_shell_runtime_config() -> io::Result<RuntimeConfig> {
    RuntimeConfig::load()
}

fn runtime_process_is_running(config: &RuntimeConfig) -> io::Result<bool> {
    let storage = StorageLayout::from_directories(&config.directories);
    let candidate_pids = runtime_process_ids(&storage)?;
    if candidate_pids.is_empty() {
        return Ok(false);
    }

    let system = System::new_all();
    Ok(candidate_pids
        .into_iter()
        .filter_map(|pid| system.process(Pid::from_u32(pid)))
        .any(process_matches_runtime_identity))
}

fn process_matches_runtime_identity(process: &sysinfo::Process) -> bool {
    let process_name = process.name().to_ascii_lowercase();
    let command_line = process.cmd().join(" ").to_ascii_lowercase();

    process_identity_matches_runtime(&process_name, &command_line)
}

fn process_identity_matches_runtime(
    process_name: &str,
    command_line: &str,
) -> bool {
    let normalized_name = process_name.trim().to_ascii_lowercase();
    let normalized_command_line = command_line.trim().to_ascii_lowercase();

    if normalized_name.contains(RUNTIME_BINARY_NAME) {
        return true;
    }

    normalized_name.contains("cargo")
        && normalized_command_line.contains(RUNTIME_BINARY_NAME)
        && normalized_command_line.contains("supervise")
}

fn runtime_process_ids(storage: &StorageLayout) -> io::Result<Vec<u32>> {
    let mut pids = Vec::new();

    match read_supervisor_snapshot(&storage.supervisor_state_path) {
        Ok(snapshot)
            if matches!(
                snapshot.status,
                RuntimeSupervisorStatus::Starting
                    | RuntimeSupervisorStatus::Running
                    | RuntimeSupervisorStatus::Restarting
            ) => {
                pids.push(snapshot.supervisor_process_id);
                if let Some(active_child_process_id) = snapshot.active_child_process_id {
                    pids.push(active_child_process_id);
                }
            }
        Ok(_) => {}
        Err(error) if is_ignorable_runtime_state_error(&error) => {}
        Err(error) => return Err(error),
    }

    match read_health_report(&storage.health_report_path) {
        Ok(report)
            if matches!(
                report.status,
                RuntimeStatus::Bootstrapping
                    | RuntimeStatus::Healthy
                    | RuntimeStatus::ShuttingDown
            ) => {
                pids.push(report.process_id);
            }
        Ok(_) => {}
        Err(error) if is_ignorable_runtime_state_error(&error) => {}
        Err(error) => return Err(error),
    }

    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
}

fn is_ignorable_runtime_state_error(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidData)
}

fn runtime_shutdown_grace_period_millis() -> u64 {
    RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS * RUNTIME_SHUTDOWN_WAIT_POLLS as u64
}

fn runtime_crash_recovery_status(
    snapshot: Option<&RuntimeSupervisorSnapshot>,
) -> String {
    match snapshot.map(|snapshot| snapshot.status) {
        Some(RuntimeSupervisorStatus::Starting | RuntimeSupervisorStatus::Running) => {
            String::from("supervisor_running")
        }
        Some(RuntimeSupervisorStatus::Restarting) => String::from("recovering"),
        Some(RuntimeSupervisorStatus::Completed) => String::from("runtime_stopped_cleanly"),
        Some(RuntimeSupervisorStatus::Failed) => String::from("restart_policy_exhausted"),
        None => String::from("not_started"),
    }
}

fn window_transition_settings() -> WindowTransitionSettings {
    WindowTransitionSettings::current()
}

fn animate_main_window_focus_transition(
    app_handle: &AppHandle,
    target: WindowFocusTarget,
) -> Result<(), String> {
    let transition = window_transition_settings();
    let window = main_window(app_handle)?;
    let start_size = window.outer_size().map_err(|error| error.to_string())?;
    let start_layout = WindowLayoutPreset::new(start_size.width, start_size.height);
    let target_layout = transition.target_layout(target);

    if start_layout == target_layout {
        return apply_main_window_layout(app_handle, target_layout);
    }

    let total_steps = transition.animation_steps();
    let step_sleep = transition.duration_millis / total_steps as u64;
    let started_at = Instant::now();

    for step in 1..=total_steps {
        let next_layout = WindowLayoutPreset::new(
            interpolate_dimension(
                start_layout.width,
                target_layout.width,
                step,
                total_steps,
            ),
            interpolate_dimension(
                start_layout.height,
                target_layout.height,
                step,
                total_steps,
            ),
        );
        apply_main_window_layout(app_handle, next_layout)?;

        if step == total_steps || step_sleep == 0 {
            continue;
        }

        let elapsed = started_at.elapsed();
        let scheduled_elapsed = Duration::from_millis(step_sleep * step as u64);
        if scheduled_elapsed > elapsed {
            thread::sleep(scheduled_elapsed - elapsed);
        }
    }

    Ok(())
}

fn interpolate_dimension(start: u32, end: u32, step: u32, total_steps: u32) -> u32 {
    if total_steps == 0 {
        return end;
    }

    let start = start as i64;
    let end = end as i64;
    let delta = end - start;

    (start + (delta * step as i64) / total_steps as i64) as u32
}

fn current_runtime_command_plan(action: RuntimeLaunchAction) -> io::Result<RuntimeCommandPlan> {
    if cfg!(debug_assertions) {
        return Ok(development_runtime_command_plan(
            &workspace_root(),
            &development_cargo_path(),
            action,
        ));
    }

    packaged_runtime_command_plan(&std::env::current_exe()?, action)
}

fn development_runtime_command_plan(
    workspace_root: &Path,
    cargo_path: &Path,
    action: RuntimeLaunchAction,
) -> RuntimeCommandPlan {
    RuntimeCommandPlan {
        program: cargo_path.to_path_buf(),
        args: vec![
            String::from("run"),
            String::from("-p"),
            String::from(RUNTIME_PACKAGE_NAME),
            String::from("--bin"),
            String::from(RUNTIME_BINARY_NAME),
            String::from("--"),
            String::from(action.as_arg()),
        ],
        current_dir: Some(workspace_root.to_path_buf()),
        inherit_stdio: true,
    }
}

fn packaged_runtime_command_plan(
    current_executable: &Path,
    action: RuntimeLaunchAction,
) -> io::Result<RuntimeCommandPlan> {
    let parent = current_executable.parent().ok_or_else(|| {
        io::Error::new(
            ErrorKind::NotFound,
            format!(
                "desktop shell executable {} has no parent directory",
                current_executable.display()
            ),
        )
    })?;

    let program = parent.join(runtime_binary_file_name());
    if !program.is_file() {
        return Err(io::Error::new(
            ErrorKind::NotFound,
            format!(
                "packaged runtime executable was not found at {}",
                program.display()
            ),
        ));
    }

    Ok(RuntimeCommandPlan {
        program,
        args: vec![String::from(action.as_arg())],
        current_dir: None,
        inherit_stdio: false,
    })
}

fn development_cargo_path() -> PathBuf {
    option_env!("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cargo"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn runtime_binary_file_name() -> String {
    format!("{RUNTIME_BINARY_NAME}{}", std::env::consts::EXE_SUFFIX)
}

fn butler_binary_file_name() -> String {
    format!("{BUTLER_BINARY_NAME}{}", std::env::consts::EXE_SUFFIX)
}

fn apply_runtime_command_environment(
    command: &mut Command,
    runtime_root: &Path,
    butler_sidecar_path: Option<&Path>,
) {
    command.env(RUNTIME_ROOT_ENV, runtime_root);
    if let Some(butler_sidecar_path) = butler_sidecar_path {
        command.env(HGP_BUTLER_PATH_ENV, butler_sidecar_path);
    }
}

fn current_butler_sidecar_path() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return development_butler_sidecar_path(&workspace_root());
    }

    packaged_butler_sidecar_path(&std::env::current_exe().ok()?)
}

fn development_butler_sidecar_path(workspace_root: &Path) -> Option<PathBuf> {
    let target_triple = current_desktop_target_triple()?;
    let sidecar_path = workspace_root
        .join("apps")
        .join("desktop")
        .join("src-tauri")
        .join("bin")
        .join(format!(
            "{BUTLER_BINARY_NAME}-{target_triple}{}",
            std::env::consts::EXE_SUFFIX,
        ));

    sidecar_path.is_file().then_some(sidecar_path)
}

fn packaged_butler_sidecar_path(current_executable: &Path) -> Option<PathBuf> {
    let sidecar_path = current_executable.parent()?.join(butler_binary_file_name());
    sidecar_path.is_file().then_some(sidecar_path)
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("x86_64-pc-windows-msvc");

#[cfg(all(target_os = "windows", target_arch = "aarch64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("aarch64-pc-windows-msvc");

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("x86_64-unknown-linux-gnu");

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("aarch64-unknown-linux-gnu");

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("x86_64-apple-darwin");

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = Some("aarch64-apple-darwin");

#[cfg(not(any(
    all(target_os = "windows", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "aarch64"),
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "linux", target_arch = "aarch64"),
    all(target_os = "macos", target_arch = "x86_64"),
    all(target_os = "macos", target_arch = "aarch64"),
)))]
const CURRENT_DESKTOP_TARGET_TRIPLE: Option<&str> = None;

fn current_desktop_target_triple() -> Option<&'static str> {
    CURRENT_DESKTOP_TARGET_TRIPLE
}

#[cfg(test)]
mod tests {
    use crate::load_retained_log_archive_entry;
    use super::{
        ActiveSystemDialogGuard,
        apply_runtime_command_environment,
        butler_binary_file_name,
        development_butler_sidecar_path,
        finalize_github_auth_login_with_known_accounts,
        github_browser_login_command_args,
        github_auth_credential_config_json,
        select_github_auth_login,
        load_artifact_inspection,
        load_build_execution_report,
        load_build_history,
        load_process_feed,
        OnDemandReleaseProcessCommandInput,
        request_on_demand_release_process,
        request_release_process_rerun,
        load_repository_inspection,
        load_repository_project_detail,
        load_release_status,
        load_localization_settings_from_paths,
        localization_preferences_path,
        development_runtime_command_plan, load_runtime_directory_settings,
        load_runtime_health_report, load_runtime_lifecycle_settings,
        load_runtime_log_lines,
        detect_repository_provider,
        EVENT_TOPIC_RELEASE_QUEUED,
        has_active_system_dialogs,
        is_main_window_focus_loss_suppressed,
        normalize_external_url,
        runtime_process_ids,
        persist_repository_auth_assessment,
        persist_repository_auth_connect,
        persist_repository_auth_disconnect,
        persist_repository_auth_reconnect,
        persist_repository_project,
        persist_repository_project_removal,
        persist_repository_project_update,
        persist_localization_preferences_to_paths,
        persist_publish_target_secret_binding,
        persist_secret_credential,
        process_identity_matches_runtime,
        purge_build_execution_retention_files,
        packaged_butler_sidecar_path,
        load_secret_settings,
        PersistedLocalizationPreferences,
        resolve_github_auth_credential,
        ShellLifecycleState,
        should_hide_main_window_on_focus_loss_state,
        load_unity_adapter_settings,
        normalize_runtime_log_line_limit, packaged_runtime_command_plan,
        runtime_binary_file_name, RuntimeLaunchAction, BUTLER_BINARY_NAME,
        HGP_BUTLER_PATH_ENV, RUNTIME_BINARY_NAME,
        BuildContractCommandInput,
        CreateRepositoryProjectBuildTargetCommandInput,
        CreateRepositoryProjectCommandInput,
        ProcessFeedInput,
        RemoveRepositoryProjectCommandInput,
        RepositoryAccessAssessment,
        RepositoryAccessAssessmentInput,
        RepositoryProviderDetection,
        AUTH_PROVIDER_STATUS_CONNECTED,
        UpdateRepositoryProjectBuildTargetCommandInput,
        UpdateRepositoryProjectCommandInput,
        ConnectRepositoryAuthInput, DisconnectRepositoryAuthInput,
        ReconnectRepositoryAuthInput, SaveLocalizationPreferencesInput,
        SaveSecretCredentialInput,
        SyncRepositoryAuthAssessmentInput,
        UpdatePublishTargetSecretBindingInput, UnityBuildContractCommandInput,
        window_transition_settings,
    };
    use runtime_config::{RuntimeConfig, RUNTIME_ROOT_ENV};
    use runtime_core::{
        bootstrap_runtime, read_runtime_event_batch, write_supervisor_snapshot,
        RuntimeRestartPolicy, RuntimeStatus, RuntimeSupervisorSnapshot,
        RuntimeSupervisorStatus,
    };
    use runtime_store::{
        initialize_database, open_connection, LocalCoordinator,
        ManualReleaseDispatchInput, RemoveRepositoryProjectStrategy,
        StorageLayout,
    };
    use rusqlite::params;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    #[test]
    fn process_identity_matches_runtime_accepts_runtime_binary_name() {
        assert!(process_identity_matches_runtime(
            "hgp-runtime.exe",
            "C:/repo/target/debug/hgp-runtime.exe serve"
        ));
    }

    #[test]
    fn process_identity_matches_runtime_accepts_cargo_supervisor_command() {
        assert!(process_identity_matches_runtime(
            "cargo.exe",
            "cargo run -p runtime-bin --bin hgp-runtime -- supervise"
        ));
    }

    #[test]
    fn process_identity_matches_runtime_rejects_unrelated_reused_pid() {
        assert!(!process_identity_matches_runtime(
            "cmd.exe",
            "C:\\Windows\\System32\\cmd.exe"
        ));
    }

    #[test]
    fn development_runtime_command_plan_uses_workspace_cargo_run() {
        let plan = development_runtime_command_plan(
            Path::new("C:/repo"),
            Path::new("C:/tools/cargo.exe"),
            RuntimeLaunchAction::Supervise,
        );

        assert_eq!(plan.program, PathBuf::from("C:/tools/cargo.exe"));
        assert_eq!(plan.current_dir, Some(PathBuf::from("C:/repo")));
        assert_eq!(
            plan.args,
            vec![
                "run",
                "-p",
                "runtime-bin",
                "--bin",
                "hgp-runtime",
                "--",
                "supervise",
            ]
        );
        assert!(plan.inherit_stdio);
    }

    #[test]
    fn packaged_runtime_command_plan_uses_sibling_runtime_binary() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-plan-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }
        std::fs::create_dir_all(&root).expect("temp directory should create");

        let desktop_path = root.join(format!("HGP{}", std::env::consts::EXE_SUFFIX));
        let runtime_path = root.join(runtime_binary_file_name());
        std::fs::write(&desktop_path, b"desktop").expect("desktop binary placeholder should write");
        std::fs::write(&runtime_path, b"runtime").expect("runtime binary placeholder should write");

        let plan = packaged_runtime_command_plan(&desktop_path, RuntimeLaunchAction::Shutdown)
            .expect("packaged runtime plan should resolve sibling runtime binary");

        assert_eq!(plan.program, runtime_path);
        assert_eq!(plan.args, vec!["shutdown"]);
        assert!(!plan.inherit_stdio);

        if root.exists() {
            std::fs::remove_dir_all(root).expect("temp directory should be removable");
        }
    }

    #[test]
    fn development_butler_sidecar_path_uses_workspace_bin_layout() {
        let root = std::env::temp_dir().join("desktop-shell-butler-sidecar-dev-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let target_triple = super::current_desktop_target_triple()
            .expect("test host should map to a supported target triple");
        let sidecar_path = root
            .join("apps")
            .join("desktop")
            .join("src-tauri")
            .join("bin")
            .join(format!(
                "{BUTLER_BINARY_NAME}-{target_triple}{}",
                std::env::consts::EXE_SUFFIX,
            ));

        std::fs::create_dir_all(sidecar_path.parent().expect("sidecar path should have parent"))
            .expect("sidecar directory should create");
        std::fs::write(&sidecar_path, b"butler").expect("sidecar placeholder should write");

        assert_eq!(development_butler_sidecar_path(&root), Some(sidecar_path.clone()));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn packaged_butler_sidecar_path_uses_sibling_binary() {
        let root = std::env::temp_dir().join("desktop-shell-butler-sidecar-package-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }
        std::fs::create_dir_all(&root).expect("temp directory should create");

        let desktop_path = root.join(format!("HGP{}", std::env::consts::EXE_SUFFIX));
        let sidecar_path = root.join(butler_binary_file_name());
        std::fs::write(&desktop_path, b"desktop").expect("desktop binary placeholder should write");
        std::fs::write(&sidecar_path, b"butler").expect("butler sidecar placeholder should write");

        assert_eq!(packaged_butler_sidecar_path(&desktop_path), Some(sidecar_path.clone()));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn apply_runtime_command_environment_sets_runtime_root_and_butler_sidecar() {
        let runtime_root = if cfg!(windows) {
            PathBuf::from("C:/repo/runtime")
        } else {
            PathBuf::from("/repo/runtime")
        };
        let butler_sidecar_path = if cfg!(windows) {
            PathBuf::from("C:/repo/apps/desktop/src-tauri/bin/hgp-butler.exe")
        } else {
            PathBuf::from("/repo/apps/desktop/src-tauri/bin/hgp-butler")
        };
        let mut command = Command::new("cargo");

        apply_runtime_command_environment(
            &mut command,
            &runtime_root,
            Some(&butler_sidecar_path),
        );

        let runtime_root_env = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(RUNTIME_ROOT_ENV))
            .and_then(|(_, value)| value.map(|entry| entry.to_os_string()));
        let butler_sidecar_env = command
            .get_envs()
            .find(|(key, _)| *key == std::ffi::OsStr::new(HGP_BUTLER_PATH_ENV))
            .and_then(|(_, value)| value.map(|entry| entry.to_os_string()));

        assert_eq!(
            runtime_root_env,
            Some(runtime_root.as_os_str().to_os_string()),
        );
        assert_eq!(
            butler_sidecar_env,
            Some(butler_sidecar_path.as_os_str().to_os_string()),
        );
    }

    #[test]
    fn load_runtime_health_report_reads_persisted_runtime_snapshot() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-health-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("runtime bootstrap should persist health metadata");

        let report = load_runtime_health_report(&config)
            .expect("desktop shell health loader should read persisted report");

        assert_eq!(report.status, RuntimeStatus::Healthy);
        assert_eq!(report.health_report_path, storage.health_report_path);

        if root.exists() {
            std::fs::remove_dir_all(root).expect("temp directory should be removable");
        }
    }

    #[test]
    fn runtime_process_ids_ignores_empty_persisted_snapshot_files() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-empty-state-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        config
            .directories
            .ensure_exists()
            .expect("directories should be created");
        std::fs::write(&storage.supervisor_state_path, b"")
            .expect("empty supervisor snapshot placeholder should write");
        std::fs::write(&storage.health_report_path, b"")
            .expect("empty health report placeholder should write");

        assert_eq!(
            runtime_process_ids(&storage).expect("empty state files should be ignored"),
            Vec::<u32>::new(),
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_runtime_log_lines_reads_recent_runtime_events() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-logs-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            RuntimeRestartPolicy::from_settings(&config.supervision),
        )
        .expect("runtime bootstrap should persist log metadata");

        let logs = load_runtime_log_lines(&config, 2)
            .expect("desktop shell log loader should read persisted log lines");

        assert_eq!(logs.len(), 2);
        assert!(logs[0].contains("\"event\":\"runtime.bootstrap.started\""));
        assert!(logs[1].contains("\"event\":\"runtime.bootstrap.completed\""));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn normalize_runtime_log_line_limit_clamps_requested_window() {
        assert_eq!(normalize_runtime_log_line_limit(None), 100);
        assert_eq!(normalize_runtime_log_line_limit(Some(0)), 1);
        assert_eq!(normalize_runtime_log_line_limit(Some(999)), 500);
    }

    #[test]
    fn normalize_external_url_accepts_http_and_https_targets() {
        assert_eq!(
            normalize_external_url("https://indiegabo.github.io/handy-games-publisher/")
                .expect("https URL should be accepted"),
            "https://indiegabo.github.io/handy-games-publisher/"
        );
        assert_eq!(
            normalize_external_url("http://localhost:4173/docs")
                .expect("http URL should be accepted"),
            "http://localhost:4173/docs"
        );
    }

    #[test]
    fn normalize_external_url_rejects_blank_and_unsupported_targets() {
        assert!(normalize_external_url("   ").is_err());
        assert!(normalize_external_url("ftp://example.com").is_err());
        assert!(normalize_external_url("https://example.com docs").is_err());
    }

    #[test]
    fn window_transition_settings_expand_focus_mode_from_main_preset() {
        let settings = window_transition_settings();

        assert_eq!(settings.main.width, 360);
        assert_eq!(settings.main.height, 420);
        assert_eq!(settings.focus.width, 540);
        assert_eq!(settings.focus.height, 840);
        assert_eq!(settings.duration_millis, 150);
    }

    #[test]
    fn focus_loss_policy_hides_only_when_window_is_unpinned_and_idle() {
        assert!(should_hide_main_window_on_focus_loss_state(
            true,
            false,
            false,
            false,
        ));
        assert!(!should_hide_main_window_on_focus_loss_state(
            false,
            false,
            false,
            false,
        ));
        assert!(!should_hide_main_window_on_focus_loss_state(
            true,
            true,
            false,
            false,
        ));
        assert!(!should_hide_main_window_on_focus_loss_state(
            true,
            false,
            true,
            false,
        ));
        assert!(!should_hide_main_window_on_focus_loss_state(
            true,
            false,
            false,
            true,
        ));
    }

    #[test]
    fn system_dialog_guard_keeps_focus_loss_suppressed_after_last_dialog_closes() {
        let lifecycle = ShellLifecycleState::default();

        {
            let _dialog_guard = ActiveSystemDialogGuard::acquire(&lifecycle)
                .expect("system dialog guard should acquire");

            assert!(has_active_system_dialogs(&lifecycle));
            assert!(!is_main_window_focus_loss_suppressed(&lifecycle));
        }

        assert!(!has_active_system_dialogs(&lifecycle));
        assert!(is_main_window_focus_loss_suppressed(&lifecycle));
    }

    #[test]
    fn load_runtime_directory_settings_resolves_runtime_paths() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-directories-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let settings = load_runtime_directory_settings(&config)
            .expect("desktop shell directory loader should resolve runtime paths");

        assert_eq!(settings.data_dir, root);
        assert_eq!(settings.state_dir, settings.data_dir.join("state"));
        assert_eq!(settings.logs_dir, settings.data_dir.join("logs"));
        assert_eq!(settings.artifacts_dir, settings.data_dir.join("artifacts"));
        assert_eq!(settings.runs_dir, settings.data_dir.join("runs"));
        assert_eq!(settings.database_path, settings.state_dir.join("runtime.db"));
        assert_eq!(
            settings.health_report_path,
            settings.state_dir.join("health.json")
        );
        assert_eq!(
            settings.supervision_contract_path,
            settings.state_dir.join("supervision.json")
        );
        assert_eq!(
            settings.supervisor_state_path,
            settings.state_dir.join("supervisor-state.json")
        );
        assert_eq!(
            settings.runtime_events_path,
            settings.state_dir.join("runtime-events.jsonl")
        );
        assert_eq!(
            settings.runtime_events_cursor_path,
            settings.state_dir.join("runtime-events.cursor.json")
        );
        assert_eq!(settings.runtime_log_path, settings.logs_dir.join("runtime.jsonl"));
        assert!(settings.data_dir.is_dir());
        assert!(settings.state_dir.is_dir());
        assert!(settings.logs_dir.is_dir());
        assert!(settings.artifacts_dir.is_dir());
        assert!(settings.runs_dir.is_dir());

        std::fs::remove_dir_all(&settings.data_dir)
            .expect("temp directory should be removable");
    }

    #[test]
    fn load_localization_settings_from_paths_discovers_official_locale_packs() {
        let root = std::env::temp_dir().join("desktop-shell-localization-settings-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let localization_root = root.join("localizations");
        let settings_dir = root.join("config");
        std::fs::create_dir_all(&localization_root)
            .expect("localization root should create");
        std::fs::write(
            localization_root.join("en.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "display_name": "English",
                "native_name": "English",
                "messages": {
                    "settings.language.title": "Language"
                }
            }))
            .expect("english locale should serialize"),
        )
        .expect("english locale should write");
        std::fs::write(
            localization_root.join("pt-BR.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "display_name": "Portuguese (Brazil)",
                "native_name": "Portugues (Brasil)",
                "messages": {
                    "settings.language.title": "Idioma"
                }
            }))
            .expect("portuguese locale should serialize"),
        )
        .expect("portuguese locale should write");

        let settings = load_localization_settings_from_paths(
            &localization_root,
            &settings_dir,
        )
        .expect("localization settings should load");

        assert_eq!(settings.localization_root, localization_root);
        assert_eq!(settings.primary_locale, "en");
        assert_eq!(settings.fallback_locale, "pt-BR");
        assert_eq!(settings.available_locales.len(), 2);
        assert_eq!(settings.available_locales[0].code, "en");
        assert_eq!(settings.available_locales[1].code, "pt-BR");
        assert!(settings.available_locales.iter().all(|locale| locale.is_official));
        assert!(settings.warnings.is_empty());

        std::fs::remove_dir_all(&root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_localization_preferences_to_paths_round_trips_selected_locales() {
        let root = std::env::temp_dir().join("desktop-shell-localization-persist-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let localization_root = root.join("localizations");
        let settings_dir = root.join("config");
        std::fs::create_dir_all(&localization_root)
            .expect("localization root should create");
        std::fs::write(
            localization_root.join("en.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "display_name": "English",
                "messages": {
                    "settings.language.title": "Language"
                }
            }))
            .expect("english locale should serialize"),
        )
        .expect("english locale should write");
        std::fs::write(
            localization_root.join("es.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "display_name": "Spanish",
                "native_name": "Espanol",
                "messages": {
                    "settings.language.title": "Idioma"
                }
            }))
            .expect("spanish locale should serialize"),
        )
        .expect("spanish locale should write");
        std::fs::write(
            localization_root.join("broken.json"),
            b"{not-json",
        )
        .expect("broken locale should write");

        let settings = persist_localization_preferences_to_paths(
            &localization_root,
            &settings_dir,
            SaveLocalizationPreferencesInput {
                primary_locale: String::from("es"),
                fallback_locale: String::from("en"),
            },
        )
        .expect("localization preferences should persist");

        assert_eq!(settings.primary_locale, "es");
        assert_eq!(settings.fallback_locale, "en");
        assert_eq!(settings.available_locales.len(), 2);
        assert_eq!(settings.available_locales[0].code, "en");
        assert_eq!(settings.available_locales[1].code, "es");
        assert!(!settings.available_locales[1].is_official);
        assert!(settings.warnings.iter().any(|warning| warning.contains("broken.json")));

        let persisted = std::fs::read(localization_preferences_path(&settings_dir))
            .expect("localization preferences file should exist");
        let persisted = serde_json::from_slice::<PersistedLocalizationPreferences>(&persisted)
            .expect("persisted localization preferences should deserialize");
        assert_eq!(persisted.primary_locale, "es");
        assert_eq!(persisted.fallback_locale, "en");

        std::fs::remove_dir_all(&root).expect("temp directory should be removable");
    }

    #[test]
    fn load_runtime_lifecycle_settings_reports_supervisor_recovery_policy() {
        let root = std::env::temp_dir().join("desktop-shell-runtime-lifecycle-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        let restart_policy = RuntimeRestartPolicy::from_settings(&config.supervision);
        bootstrap_runtime(
            &config,
            &storage,
            Path::new(RUNTIME_BINARY_NAME),
            restart_policy.clone(),
        )
        .expect("runtime bootstrap should persist lifecycle metadata");

        let snapshot = RuntimeSupervisorSnapshot::new(
            &config,
            4100,
            Some(4200),
            2,
            1,
            Some(restart_policy.recoverable_exit_code),
            RuntimeSupervisorStatus::Restarting,
            "recoverable exit detected, retry in progress",
        )
        .expect("supervisor snapshot should build");
        write_supervisor_snapshot(&storage, &snapshot)
            .expect("supervisor snapshot should persist");

        let settings = load_runtime_lifecycle_settings(&config)
            .expect("runtime lifecycle settings should load");

        assert!(settings.shell_launches_supervisor_on_startup);
        assert!(settings.shell_requests_shutdown_on_exit);
        assert!(settings.shell_force_kills_after_timeout);
        assert!(settings.shell_relaunches_supervisor_on_restart);
        assert!(settings.runtime_supervisor_owns_crash_recovery);
        assert_eq!(settings.shutdown_grace_period_millis, 2_000);
        assert_eq!(settings.restart_policy, restart_policy);
        assert_eq!(settings.startup_command.args.last(), Some(&String::from("supervise")));
        assert_eq!(settings.shutdown_command.args.last(), Some(&String::from("shutdown")));
        assert_eq!(settings.crash_recovery_status, "recovering");
        assert_eq!(settings.supervisor_snapshot, Some(snapshot));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_release_status_reports_pending_release_backlog() {
        let root = std::env::temp_dir().join("desktop-shell-release-status-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["release-status-repo", "https://example.com/release-status.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        drop(connection);

        LocalCoordinator::new(&storage)
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id,
                git_tag: String::from("v9.0.0"),
                git_commit: String::from("deadbeef"),
                requested_via: String::from("desktop-shell-test"),
            })
            .expect("manual release dispatch should succeed");

        let snapshot = load_release_status(&config)
            .expect("release status should load queued automation snapshot");

        assert!(!snapshot.generated_at.is_empty());
        assert_eq!(snapshot.repositories.len(), 1);
        assert_eq!(snapshot.repositories[0].repository_name, "release-status-repo");
        assert_eq!(snapshot.repositories[0].enabled_build_target_count, 1);
        assert_eq!(snapshot.repositories[0].pending_release_count, 1);
        assert_eq!(snapshot.repositories[0].release_queue.len(), 1);
        assert_eq!(snapshot.repositories[0].release_queue[0].git_tag, "v9.0.0");

        let release_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == "release-runs")
            .expect("release queue snapshot should exist");
        assert_eq!(release_queue.ready_count, 1);
        assert_eq!(release_queue.leased_count, 0);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn request_release_process_rerun_reuses_selected_release_run() {
        let root = std::env::temp_dir().join("desktop-shell-release-rerun-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["release-rerun-repo", "https://example.com/release-rerun.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        drop(connection);

        let initial_release = LocalCoordinator::new(&storage)
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id,
                git_tag: String::from("v9.1.0"),
                git_commit: String::from("deadbeef"),
                requested_via: String::from("desktop-shell-test"),
            })
            .expect("manual release dispatch should succeed");

        let rerun_release = request_release_process_rerun(&config, initial_release.id)
            .expect("release rerun request should succeed");

        assert_eq!(rerun_release.id, initial_release.id);
        assert_eq!(rerun_release.repository_id, repository_id);
        assert_eq!(rerun_release.git_tag, "v9.1.0");
        assert_eq!(rerun_release.git_commit.as_deref(), Some("deadbeef"));
        assert!(rerun_release.source_metadata_json.contains("desktop-shell-ui"));

        let queued_event = latest_runtime_event(&storage);
        assert_eq!(queued_event.topic, EVENT_TOPIC_RELEASE_QUEUED);
        assert_eq!(queued_event.origin, "desktop-shell");
        assert_eq!(queued_event.release_run_id, Some(rerun_release.id));
        assert_eq!(queued_event.payload["requested_via"], "desktop-shell-ui");
        assert_eq!(queued_event.payload["source_kind"], "managed_tag");
        assert_eq!(
            queued_event.summary,
            "On-demand tag release queued for release-rerun-repo v9.1.0"
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn request_on_demand_release_process_dispatches_managed_ref_release() {
        let root = std::env::temp_dir().join("desktop-shell-on-demand-managed-ref-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind, default_branch) VALUES (?, ?, ?, ?)",
                params![
                    "on-demand-managed-ref-repo",
                    "https://example.com/on-demand-managed-ref.git",
                    "unity",
                    "main",
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        drop(connection);

        let record = request_on_demand_release_process(
            &config,
            OnDemandReleaseProcessCommandInput {
                repository_id,
                release_version: Some(String::from("v9.2.0")),
                version_source: String::from("manual"),
                source_kind: String::from("managed_ref"),
                source_ref: Some(String::from("release/next")),
                local_path: None,
                unity_executable_path_override: Some(String::from("  C:/Unity/Editor/Unity.exe  ")),
            },
        )
        .expect("on-demand managed-ref dispatch should succeed");

        assert_eq!(record.repository_id, repository_id);
        assert_eq!(record.git_tag, "v9.2.0");
        assert_eq!(record.trigger_source, "manual");
        let metadata: serde_json::Value = serde_json::from_str(&record.source_metadata_json)
            .expect("source metadata should decode");
        assert_eq!(metadata["requested_via"], "desktop-shell-ui");
        assert_eq!(metadata["source_kind"], "managed_ref");
        assert_eq!(metadata["source_ref"], "release/next");
        assert_eq!(
            metadata["unity_executable_path_override"],
            "C:/Unity/Editor/Unity.exe"
        );

        let queued_event = latest_runtime_event(&storage);
        assert_eq!(queued_event.topic, EVENT_TOPIC_RELEASE_QUEUED);
        assert_eq!(queued_event.origin, "desktop-shell");
        assert_eq!(queued_event.release_run_id, Some(record.id));
        assert_eq!(queued_event.payload["source_kind"], "managed_ref");
        assert_eq!(queued_event.payload["source_ref"], "release/next");
        assert_eq!(
            queued_event.summary,
            "On-demand ref release queued for on-demand-managed-ref-repo v9.2.0"
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn request_on_demand_release_process_dispatches_local_workspace_release() {
        let root = std::env::temp_dir().join("desktop-shell-on-demand-local-workspace-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let local_workspace = root.join("local-project");
        std::fs::create_dir_all(&local_workspace)
            .expect("local workspace directory should create");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params![
                    "on-demand-local-workspace-repo",
                    "https://example.com/on-demand-local-workspace.git",
                    "unity",
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        drop(connection);

        let record = request_on_demand_release_process(
            &config,
            OnDemandReleaseProcessCommandInput {
                repository_id,
                release_version: Some(String::from("v9.3.0")),
                version_source: String::from("manual"),
                source_kind: String::from("local_workspace"),
                source_ref: None,
                local_path: Some(local_workspace.display().to_string()),
                unity_executable_path_override: None,
            },
        )
        .expect("on-demand local-workspace dispatch should succeed");

        assert_eq!(record.repository_id, repository_id);
        assert_eq!(record.git_tag, "v9.3.0");
        assert_eq!(record.trigger_source, "manual");
        let metadata: serde_json::Value = serde_json::from_str(&record.source_metadata_json)
            .expect("source metadata should decode");
        assert_eq!(metadata["requested_via"], "desktop-shell-ui");
        assert_eq!(metadata["source_kind"], "local_workspace");
        let persisted_local_path = PathBuf::from(
            metadata["local_path"]
                .as_str()
                .expect("local workspace path should serialize as a string"),
        );
        assert_eq!(persisted_local_path, local_workspace);

        let queued_event = latest_runtime_event(&storage);
        assert_eq!(queued_event.topic, EVENT_TOPIC_RELEASE_QUEUED);
        assert_eq!(queued_event.origin, "desktop-shell");
        assert_eq!(queued_event.release_run_id, Some(record.id));
        assert_eq!(queued_event.payload["source_kind"], "local_workspace");
        let event_local_path = PathBuf::from(
            queued_event.payload["local_path"]
                .as_str()
                .expect("event local path should serialize as a string"),
        );
        assert_eq!(event_local_path, local_workspace);
        assert_eq!(
            queued_event.summary,
            "On-demand local release queued for on-demand-local-workspace-repo v9.3.0"
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    fn latest_runtime_event(storage: &StorageLayout) -> runtime_core::RuntimeEventRecord {
        read_runtime_event_batch(&storage.runtime_events_path, 0)
            .expect("runtime event stream should load")
            .events
            .into_iter()
            .last()
            .expect("runtime event stream should contain at least one event")
    }

    #[test]
    fn load_repository_inspection_reports_repository_config_and_backlog() {
        let root = std::env::temp_dir().join("desktop-shell-repository-inspection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("repository credentials should insert");
        let repository_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "itch-release-token",
                    "itch-api-key",
                    r#"{"api_key":"top-secret-token"}"#,
                ],
            )
            .expect("publish credentials should insert");
        let publish_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    repo_url,
                    credentials_id,
                    polling_interval_seconds
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    "repo-inspection",
                    "https://example.com/repo-inspection.git",
                    repository_credentials_id,
                    120_i64,
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    config_json,
                    credentials_id
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "itch-release",
                    "itch",
                    r#"{"account_name":"indiegabo","game_slug":"revolutions"}"#,
                    publish_credentials_id,
                ],
            )
            .expect("itch publish target should insert");
        let itch_publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    config_json,
                    credentials_id
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "filesystem-release",
                    "filesystem",
                    r#"{"root_path":"D:/published"}"#,
                    Option::<i64>::None,
                ],
            )
            .expect("publish target should insert");
        let filesystem_publish_target_id = connection.last_insert_rowid();
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
                params![
                    build_target_id,
                    filesystem_publish_target_id,
                    1_i64,
                    r#"{"operation":"move","directory_path":"D:/published"}"#,
                ],
            )
            .expect("filesystem binding should insert");
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
                params![
                    build_target_id,
                    itch_publish_target_id,
                    1_i64,
                    r#"{"channel":"windows-stable","userversion_template":"release-{{git_tag}}"}"#,
                ],
            )
            .expect("itch binding should insert");
        drop(connection);

        LocalCoordinator::new(&storage)
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id,
                git_tag: String::from("v10.0.0"),
                git_commit: String::from("feedface"),
                requested_via: String::from("desktop-shell-test"),
            })
            .expect("manual release dispatch should succeed");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should aggregate repository metadata");
        let detail = load_repository_project_detail(&config, repository_id)
            .expect("repository detail should load one repository without host capability discovery");

        assert!(!inspection.generated_at.is_empty());
        assert_eq!(inspection.repositories.len(), 1);

        let repository = &inspection.repositories[0];
        assert_eq!(repository.repository_name, "repo-inspection");
        assert_eq!(repository.repo_url, "https://example.com/repo-inspection.git");
        assert_eq!(repository.polling_interval_seconds, 120);
        assert_eq!(repository.enabled_build_target_count, 1);
        assert_eq!(repository.build_targets.len(), 1);
        assert_eq!(repository.build_targets[0].target_name, "windows-player");
        assert_eq!(repository.publish_targets.len(), 2);
        assert_eq!(repository.pending_release_count, 1);
        assert_eq!(repository.release_queue.len(), 1);
        assert_eq!(repository.release_queue[0].git_tag, "v10.0.0");
        assert_eq!(detail.repository_id, repository_id);
        assert_eq!(detail.repository_name, "repo-inspection");
        assert_eq!(detail.publish_targets.len(), 2);
        assert_eq!(detail.release_queue.len(), 1);
        assert_eq!(detail.release_queue[0].git_tag, "v10.0.0");

        let repository_credentials = repository
            .credentials
            .as_ref()
            .expect("repository credentials should resolve");
        assert_eq!(repository_credentials.name, "origin-basic");
        assert_eq!(repository_credentials.kind, "git-http-basic");
        assert_eq!(repository_credentials.config_status, "ready");

        let itch_target = repository
            .publish_targets
            .iter()
            .find(|target| target.name == "itch-release")
            .expect("itch destination should be present");
        assert_eq!(itch_target.kind, "itch");
        assert_eq!(
            itch_target.config_json,
            r#"{"account_name":"indiegabo","game_slug":"revolutions"}"#
        );
        assert_eq!(itch_target.bindings.len(), 1);
        assert_eq!(itch_target.bindings[0].build_target_name, "windows-player");
        assert_eq!(itch_target.bindings[0].consumption_behavior, "non_consuming");

        let publish_credentials = itch_target
            .credentials
            .as_ref()
            .expect("publish target credentials should resolve");
        assert_eq!(publish_credentials.name, "itch-release-token");
        assert_eq!(publish_credentials.kind, "itch-api-key");
        assert_eq!(publish_credentials.config_status, "ready");

        let filesystem_target = repository
            .publish_targets
            .iter()
            .find(|target| target.name == "filesystem-release")
            .expect("filesystem destination should be present");
        assert_eq!(filesystem_target.kind, "filesystem");
        assert_eq!(
            filesystem_target.config_json,
            r#"{"root_path":"D:/published"}"#
        );
        assert!(filesystem_target.credentials.is_none());
        assert_eq!(filesystem_target.bindings.len(), 1);
        assert_eq!(
            filesystem_target.bindings[0].consumption_behavior,
            "consuming"
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_build_history_reports_recent_build_activity() {
        let root = std::env::temp_dir().join("desktop-shell-build-history-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["build-history-repo", "https://example.com/build-history.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v11.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    engine_version,
                    image_ref,
                    status,
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    started_at,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "succeeded",
                    "C:/runs/build-history",
                    "C:/logs/build-history.log",
                    "C:/artifacts/build-history",
                    "2026-01-11T10:00:00Z",
                    "2026-01-11T10:05:00Z",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO artifacts (build_run_id, name, kind, path) VALUES (?, ?, ?, ?)",
                params![build_run_id, "history.zip", "archive", "artifacts/history.zip"],
            )
            .expect("artifact should insert");
        let artifact_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    "queued",
                ],
            )
            .expect("publish run should insert");
        drop(connection);

        let builds = load_build_history(&config)
            .expect("build history should load recent build activity");

        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0].build_run_id, build_run_id);
        assert_eq!(builds[0].release_run_id, release_run_id);
        assert_eq!(builds[0].repository_name, "build-history-repo");
        assert_eq!(builds[0].git_tag, "v11.0.0");
        assert_eq!(builds[0].build_target_name, "windows-player");
        assert_eq!(builds[0].status, "succeeded");
        assert_eq!(builds[0].artifact_count, 1);
        assert_eq!(builds[0].publish_run_count, 1);
        assert_eq!(
            builds[0].log_path.as_deref(),
            Some("C:/logs/build-history.log")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_process_feed_reports_repository_engine_identity() {
        let root = std::env::temp_dir().join("desktop-shell-process-feed-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind, polling_interval_seconds) VALUES (?, ?, ?, ?)",
                params![
                    "process-feed-repo",
                    "https://example.com/process-feed.git",
                    "unity",
                    300_i64,
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"StandaloneWindows64","buildMethod":"CI.Build.Perform","editorVersion":"2022.3.20f1"}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        drop(connection);

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v18.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "queued",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    engine_version,
                    image_ref,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "queued",
                ],
            )
            .expect("queued build run should insert");
        drop(connection);

        let feed = load_process_feed(&config, ProcessFeedInput::default())
            .expect("process feed should load queued build activity");

        assert_eq!(feed.page, 1);
        assert_eq!(feed.page_size, 6);
        assert_eq!(feed.total_items, 1);
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].repository_id, repository_id);
        assert_eq!(feed.items[0].repository_name, "process-feed-repo");
        assert_eq!(feed.items[0].repository_engine_kind, "unity");
        assert_eq!(feed.items[0].git_tag, "v18.0.0");
        assert_eq!(feed.items[0].engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(feed.items[0].display_status, "queued");
        assert_eq!(feed.items[0].current_step_label, "Queued build: Windows");
        assert_eq!(feed.items[0].queued_build_runs, 1);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_build_execution_report_reads_retained_json_report() {
        let root = std::env::temp_dir().join("desktop-shell-build-execution-report-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let workspace_path = config.directories.runs_dir.join("build-run-report-sample");
        let retained_dir = workspace_path.join("retained");
        std::fs::create_dir_all(&retained_dir).expect("retained directory should create");
        let report_path = retained_dir.join("execution-report.json");
        let archive_path = retained_dir.join("execution-logs.zip");
        std::fs::write(
            &report_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
                "cleanup_policy": "retain-zipped-logs-json-report",
                "build_run": {
                    "status": "succeeded"
                },
                "cleanup": {
                    "status": "completed"
                }
            }))
            .expect("report json should serialize"),
        )
        .expect("report file should write");
        write_test_log_archive(
            &archive_path,
            &[
                (
                    "release-run-1/logs/03-unity-build-standalonewindows64.log",
                    "windows line 1\nwindows line 2\n",
                ),
                (
                    "release-run-1/logs/04-unity-build-standalonelinux64.log",
                    "linux line 1\nlinux line 2\n",
                ),
            ],
        );

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["report-repo", "https://example.com/report.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v16.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    workspace_path,
                    artifact_root_path,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    workspace_path.display().to_string(),
                    "C:/artifacts/build-run-report-sample",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        drop(connection);

        let payload = load_build_execution_report(&config, build_run_id)
            .expect("build execution report should load");

        assert_eq!(payload.build_run_id, build_run_id);
        assert_eq!(payload.workspace_path.as_deref(), Some(workspace_path.as_path()));
        assert_eq!(payload.retained_dir_path.as_deref(), Some(retained_dir.as_path()));
        assert_eq!(payload.report_path.as_deref(), Some(report_path.as_path()));
        assert_eq!(payload.logs_archive_path.as_deref(), Some(archive_path.as_path()));
        assert!(payload.exists);
        assert!(payload.logs_archive_exists);
        assert_eq!(payload.log_entries.len(), 2);
        assert_eq!(
            payload.log_entries[0].entry_name,
            String::from("03-unity-build-standalonewindows64.log")
        );
        assert_eq!(
            payload
                .report
                .as_ref()
                .and_then(|report| report.get("cleanup"))
                .and_then(|cleanup| cleanup.get("status"))
                .and_then(|status| status.as_str()),
            Some("completed")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_retained_log_archive_entry_reads_content_from_execution_logs_zip() {
        let root = std::env::temp_dir().join("desktop-shell-retained-log-entry-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let workspace_path = config.directories.runs_dir.join("build-run-log-entry-sample");
        let retained_dir = workspace_path.join("retained");
        std::fs::create_dir_all(&retained_dir).expect("retained directory should create");
        let archive_path = retained_dir.join("execution-logs.zip");
        write_test_log_archive(
            &archive_path,
            &[(
                "release-run-1/logs/04-unity-build-standalonelinux64.log",
                "line 1\nline 2\nline 3\n",
            )],
        );

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["log-entry-repo", "https://example.com/log-entry.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "linux-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"linux","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v18.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    workspace_path,
                    artifact_root_path,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    workspace_path.display().to_string(),
                    "C:/artifacts/build-run-log-entry-sample",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        drop(connection);

        let preview = load_retained_log_archive_entry(
            &config,
            build_run_id,
            "release-run-1/logs/04-unity-build-standalonelinux64.log",
            1024,
        )
        .expect("retained log entry preview should load");

        assert_eq!(preview.archive_path, archive_path);
        assert!(preview.exists);
        assert_eq!(
            preview.entry_path,
            String::from("release-run-1/logs/04-unity-build-standalonelinux64.log")
        );
        assert!(!preview.truncated);
        assert_eq!(preview.content, String::from("line 1\nline 2\nline 3\n"));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn purge_build_execution_retention_removes_retained_files_and_empty_workspace() {
        let root = std::env::temp_dir().join("desktop-shell-build-execution-purge-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let workspace_path = config.directories.runs_dir.join("build-run-purge-sample");
        let retained_dir = workspace_path.join("retained");
        std::fs::create_dir_all(&retained_dir).expect("retained directory should create");
        let report_path = retained_dir.join("execution-report.json");
        std::fs::write(&report_path, "{}").expect("report file should write");
        let archive_path = retained_dir.join("execution-logs.zip");
        std::fs::write(&archive_path, "zip").expect("log archive should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["purge-repo", "https://example.com/purge.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v17.0.0",
                    "deadbeef",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    status,
                    workspace_path,
                    artifact_root_path,
                    finished_at
                )
                VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "succeeded",
                    workspace_path.display().to_string(),
                    "C:/artifacts/build-run-purge-sample",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        drop(connection);

        let purge_report = purge_build_execution_retention_files(&config, build_run_id)
            .expect("retention purge should succeed");

        assert_eq!(purge_report.build_run_id, build_run_id);
        assert_eq!(purge_report.workspace_path.as_deref(), Some(workspace_path.as_path()));
        assert_eq!(purge_report.retained_dir_path.as_deref(), Some(retained_dir.as_path()));
        assert!(purge_report.workspace_removed);
        assert_eq!(
            purge_report.removed_paths,
            vec![retained_dir.clone(), workspace_path.clone()]
        );
        assert!(!report_path.exists());
        assert!(!archive_path.exists());
        assert!(!retained_dir.exists());
        assert!(!workspace_path.exists());

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    fn write_test_log_archive(archive_path: &Path, entries: &[(&str, &str)]) {
        let archive_file = std::fs::File::create(archive_path)
            .expect("log archive file should create");
        let mut archive = zip::ZipWriter::new(archive_file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for (entry_path, contents) in entries {
            archive
                .start_file(entry_path, options)
                .expect("archive entry should start");
            std::io::Write::write_all(&mut archive, contents.as_bytes())
                .expect("archive entry should write");
        }

        archive.finish().expect("archive should finish");
    }

    #[test]
    fn load_artifact_inspection_reports_recent_artifact_activity() {
        let root = std::env::temp_dir().join("desktop-shell-artifact-inspection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params![
                    "artifact-inspection-repo",
                    "https://example.com/artifact-inspection.git",
                    "unity"
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v12.0.0",
                    "feedface",
                    "2022.3.20f1",
                    "succeeded",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    engine_version,
                    image_ref,
                    status,
                    artifact_root_path
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "succeeded",
                    "C:/artifacts/artifact-inspection",
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO artifacts (
                    build_run_id,
                    name,
                    kind,
                    path,
                    size_bytes,
                    checksum_sha256
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    "artifact-inspection.zip",
                    "archive",
                    "builds/windows/artifact-inspection.zip",
                    8192_i64,
                    "deadbeef",
                ],
            )
            .expect("artifact should insert");
        let artifact_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    "succeeded",
                ],
            )
            .expect("publish run should insert");
        drop(connection);

        let artifacts = load_artifact_inspection(&config)
            .expect("artifact inspection should load recent artifact activity");

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_id, artifact_id);
        assert_eq!(artifacts[0].build_run_id, build_run_id);
        assert_eq!(artifacts[0].release_run_id, release_run_id);
        assert_eq!(artifacts[0].repository_name, "artifact-inspection-repo");
        assert_eq!(artifacts[0].git_tag, "v12.0.0");
        assert_eq!(artifacts[0].build_target_name, "windows-player");
        assert_eq!(artifacts[0].artifact_name, "artifact-inspection.zip");
        assert_eq!(artifacts[0].artifact_kind, "archive");
        assert_eq!(artifacts[0].publish_run_count, 1);
        assert_eq!(artifacts[0].succeeded_publish_runs, 1);
        assert_eq!(artifacts[0].size_bytes, Some(8192));
        assert_eq!(
            artifacts[0].artifact_root_path.as_deref(),
            Some("C:/artifacts/artifact-inspection")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_unity_adapter_settings_reports_discovery_roots_and_target_diagnostics() {
        let root = std::env::temp_dir().join("desktop-shell-unity-runner-settings-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let unity_executable_path = root.join(format!("unity{}", std::env::consts::EXE_SUFFIX));
        std::fs::create_dir_all(&root).expect("test root should create");
        std::fs::write(&unity_executable_path, b"unity")
            .expect("fake unity executable should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["unity-settings-repo", "https://example.com/unity-settings.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"windows","buildMethod":"CI.Build.Perform","editorVersion":""}}"#,
                    serde_json::json!({
                        "unity_executable_path": unity_executable_path.display().to_string(),
                        "additional_arguments": ["-silent-crashes"],
                        "environment": {
                            "UNITY_LICENSE": "redacted"
                        }
                    })
                    .to_string(),
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        drop(connection);

        let settings = load_unity_adapter_settings(&config)
            .expect("unity runner settings should load");

        assert_eq!(settings.platform, config.platform.as_str());
        assert_eq!(settings.supported_runner_families[0], "host-native");
        assert!(!settings.discovery_roots.is_empty());
        assert_eq!(settings.capability_profile.platform, config.platform.as_str());
        assert!(!settings.capability_profile.architecture.is_empty());
        assert!(!settings.capability_profile.packaging_mode.is_empty());
        assert!(!settings.capability_profile.runner_selection.status.is_empty());
        assert_eq!(settings.build_targets.len(), 1);
        assert_eq!(settings.build_targets[0].build_target_id, build_target_id);
        assert_eq!(settings.build_targets[0].repository_id, repository_id);
        assert_eq!(settings.build_targets[0].repository_name, "unity-settings-repo");
        assert_eq!(settings.build_targets[0].target_name, "windows-player");
        assert_eq!(settings.build_targets[0].unity_target_platform, "windows");
        assert_eq!(settings.build_targets[0].runner_type, "host-native");
        assert_eq!(
            settings.build_targets[0].unity_build_method.as_deref(),
            Some("CI.Build.Perform")
        );
        assert_eq!(settings.build_targets[0].diagnostic_status, "ready");
        assert!(settings.build_targets[0].host_native_diagnostics.is_some());
        let diagnostics = settings.build_targets[0]
            .host_native_diagnostics
            .as_ref()
            .expect("host-native diagnostics should exist");
        assert_eq!(
            diagnostics.unity_executable_path.as_deref(),
            Some(unity_executable_path.display().to_string().as_str())
        );
        assert!(diagnostics.unity_executable_exists);
        assert!(diagnostics.unity_executable_is_file);
        assert_eq!(diagnostics.additional_argument_count, 1);
        assert_eq!(diagnostics.environment_variable_count, 1);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn load_secret_settings_reports_redacted_credentials_and_bindings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-settings-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("git basic credentials should insert");
        let repository_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "publish-bearer",
                    "git-http-bearer",
                    r#"{"token":"top-secret-token"}"#,
                ],
            )
            .expect("git bearer credentials should insert");
        let publish_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind, credentials_id) VALUES (?, ?, ?, ?)",
                params![
                    "revolutions",
                    "https://example.com/revolutions.git",
                    "unity",
                    repository_credentials_id,
                ],
            )
            .expect("bound repository should insert");
        let bound_repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["workers", "https://example.com/workers.git", "unity"],
            )
            .expect("unbound repository should insert");
        connection
            .execute(
                "
                INSERT INTO publish_targets (repository_id, name, kind, credentials_id)
                VALUES (?, ?, ?, ?)
                ",
                params![
                    bound_repository_id,
                    "filesystem-release",
                    "filesystem",
                    publish_credentials_id,
                ],
            )
            .expect("bound publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        drop(connection);

        let settings = load_secret_settings(&config)
            .expect("secret settings should load");

        assert_eq!(
            settings.storage_model,
            "sqlite-config-json-and-keyring-references"
        );
        assert_eq!(
            settings.supported_credential_kinds,
            vec![
                "git-http-basic",
                "git-http-bearer",
                "git-http-github-host-login",
            ]
        );
        assert_eq!(settings.credentials.len(), 2);
        assert_eq!(settings.repository_bindings.len(), 2);
        assert_eq!(settings.publish_target_bindings.len(), 1);
        assert_eq!(settings.credentials[0].name, "origin-basic");
        assert_eq!(settings.credentials[0].config_summary.status, "ready");
        assert_eq!(
            settings.credentials[0].config_summary.top_level_keys,
            vec!["password", "username"]
        );
        assert_eq!(settings.credentials[0].bindings.len(), 1);
        assert_eq!(settings.credentials[0].bindings[0].binding_kind, "repository");
        assert_eq!(settings.credentials[0].bindings[0].binding_id, bound_repository_id);
        assert_eq!(settings.credentials[1].name, "publish-bearer");
        assert_eq!(settings.credentials[1].config_summary.top_level_keys, vec!["token"]);
        assert_eq!(settings.credentials[1].bindings.len(), 1);
        assert_eq!(settings.credentials[1].bindings[0].binding_kind, "publish_target");
        assert_eq!(settings.credentials[1].bindings[0].binding_id, publish_target_id);
        assert_eq!(settings.repository_bindings[0].repository_name, "revolutions");
        assert_eq!(
            settings.repository_bindings[0].credentials_id,
            Some(repository_credentials_id)
        );
        assert_eq!(settings.repository_bindings[1].repository_name, "workers");
        assert_eq!(settings.repository_bindings[1].credentials_id, None);
        assert_eq!(
            settings.publish_target_bindings[0].credentials_id,
            Some(publish_credentials_id)
        );

        let encoded = serde_json::to_string(&settings)
            .expect("secret settings should serialize without secrets leaking");
        assert!(!encoded.contains("solidarity"));
        assert!(!encoded.contains("top-secret-token"));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_secret_credential_creates_redacted_secret_settings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-save-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        persist_secret_credential(
            &config,
            SaveSecretCredentialInput {
                credential_id: None,
                name: String::from("origin-basic"),
                kind: String::from("git-http-basic"),
                config_json: String::from(
                    r#"{"username":"worker","password":"solidarity"}"#,
                ),
            },
        )
        .expect("credential should persist");

        let settings = load_secret_settings(&config)
            .expect("secret settings should load persisted credential");

        assert_eq!(settings.credentials.len(), 1);
        assert_eq!(settings.credentials[0].name, "origin-basic");
        assert_eq!(settings.credentials[0].kind, "git-http-basic");
        assert_eq!(settings.credentials[0].config_summary.status, "ready");
        let encoded = serde_json::to_string(&settings)
            .expect("secret settings should serialize without secret values");
        assert!(!encoded.contains("solidarity"));

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_auth_commands_and_publish_target_binding_update_settings() {
        let root = std::env::temp_dir().join("desktop-shell-secret-binding-save-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-rotated",
                    "git-http-basic",
                    r#"{"username":"worker","password":"new-solidarity"}"#,
                ],
            )
            .expect("replacement credentials row should insert");
        let rotated_credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["revolutions", "https://example.com/revolutions.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        drop(connection);

        persist_repository_auth_connect(
            &config,
            ConnectRepositoryAuthInput {
                repository_id,
                credentials_id,
            },
        )
        .expect("repository connect should persist");
        persist_repository_auth_reconnect(
            &config,
            ReconnectRepositoryAuthInput {
                repository_id,
                credentials_id: rotated_credentials_id,
            },
        )
        .expect("repository reconnect should persist");
        persist_publish_target_secret_binding(
            &config,
            UpdatePublishTargetSecretBindingInput {
                publish_target_id,
                credentials_id: Some(credentials_id),
            },
        )
        .expect("publish target binding should persist");

        let settings = load_secret_settings(&config)
            .expect("secret settings should reflect updated bindings");

        assert_eq!(settings.repository_bindings.len(), 1);
        assert_eq!(
            settings.repository_bindings[0].credentials_id,
            Some(rotated_credentials_id)
        );
        assert_eq!(settings.publish_target_bindings.len(), 1);
        assert_eq!(
            settings.publish_target_bindings[0].credentials_id,
            Some(credentials_id)
        );

        persist_repository_auth_disconnect(
            &config,
            DisconnectRepositoryAuthInput { repository_id },
        )
        .expect("repository disconnect should persist");

        let settings = load_secret_settings(&config)
            .expect("secret settings should reflect disconnected repository binding");
        assert_eq!(settings.repository_bindings[0].credentials_id, None);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_auth_assessment_clears_binding_when_repository_is_public() {
        let root = std::env::temp_dir().join("desktop-shell-auth-assessment-sync-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "origin-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["revolutions", "https://example.com/revolutions.git", "unity"],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        drop(connection);

        persist_repository_auth_connect(
            &config,
            ConnectRepositoryAuthInput {
                repository_id,
                credentials_id,
            },
        )
        .expect("repository connect should persist");

        persist_repository_auth_assessment(
            &config,
            SyncRepositoryAuthAssessmentInput {
                repository_id,
                repository_access_assessment: RepositoryAccessAssessment {
                    provider_id: String::from("github"),
                    provider_label: String::from("GitHub"),
                    instance_url: String::from("https://github.com"),
                    normalized_url: String::from(
                        "https://github.com/indiegabo/revolutions.git",
                    ),
                    visibility: String::from("public"),
                    auth_requirement: String::from("none"),
                    auth_status: String::from("not_required"),
                    supports_interactive_login: true,
                    message: String::from(
                        "Public repository detected through anonymous remote access.",
                    ),
                },
            },
        )
        .expect("repository assessment sync should persist");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect synced auth assessment");
        assert_eq!(inspection.repositories.len(), 1);
        assert_eq!(inspection.repositories[0].auth_binding_status, "not_required");
        assert_eq!(inspection.repositories[0].auth_requirement_status, "none");
        assert_eq!(inspection.repositories[0].visibility_status, "public");
        assert!(inspection.repositories[0].credentials.is_none());

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn detect_repository_provider_command_reports_github_metadata() {
        let detection = detect_repository_provider(RepositoryAccessAssessmentInput {
            repository_url: String::from("https://github.com/indiegabo/hgp.git"),
        })
        .expect("provider detection should succeed");

        assert_eq!(
            detection,
            RepositoryProviderDetection {
                provider_id: String::from("github"),
                provider_label: String::from("GitHub"),
                instance_url: String::from("https://github.com"),
                normalized_url: String::from("https://github.com/indiegabo/hgp.git"),
                supports_interactive_login: true,
            }
        );
    }

    #[test]
    fn persist_repository_project_creates_repository_inspection_entry() {
        let root = std::env::temp_dir().join("desktop-shell-project-create-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://example.com/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect created project");

        assert_eq!(inspection.repositories.len(), 1);
        assert_eq!(inspection.repositories[0].repository_id, created.repository_id);
        assert_eq!(inspection.repositories[0].repository_name, "Workers");
        assert_eq!(inspection.repositories[0].repo_url, "https://example.com/workers.git");
        assert_eq!(inspection.repositories[0].engine_kind, "unity");
        assert_eq!(inspection.repositories[0].polling_interval_seconds, 300);
        assert_eq!(inspection.repositories[0].build_targets.len(), 1);
        assert_eq!(inspection.repositories[0].build_targets[0].target_name, "Windows");
        assert_eq!(inspection.repositories[0].build_targets[0].diagnostic_status, "ready");

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_creates_local_workspace_inspection_entry() {
        let root = std::env::temp_dir().join("desktop-shell-project-create-local-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();
        let local_workspace = root.join("local-project");
        std::fs::create_dir_all(&local_workspace)
            .expect("local workspace directory should create");
        let local_workspace_path = local_workspace.display().to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers Local"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("local_workspace")),
                repository_url: None,
                local_path: Some(local_workspace_path.clone()),
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: None,
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("local workspace project should persist");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect created local project");

        assert_eq!(inspection.repositories.len(), 1);
        assert_eq!(inspection.repositories[0].repository_id, created.repository_id);
        assert_eq!(inspection.repositories[0].repository_name, "Workers Local");
        assert_eq!(inspection.repositories[0].source_mode, "local_workspace");
        assert_eq!(inspection.repositories[0].workspace_strategy, "direct");
        assert_eq!(
            inspection.repositories[0]
                .local_path
                .as_deref()
                .map(|path| path.replace('\\', "/")),
            Some(local_workspace_path.replace('\\', "/"))
        );
        assert_eq!(
            inspection.repositories[0].repo_url.replace('\\', "/"),
            local_workspace_path.replace('\\', "/")
        );
        assert_eq!(inspection.repositories[0].engine_kind, "unity");
        assert_eq!(inspection.repositories[0].polling_interval_seconds, 300);
        assert_eq!(inspection.repositories[0].build_targets.len(), 1);
        assert_eq!(inspection.repositories[0].build_targets[0].target_name, "Windows");
        assert_eq!(inspection.repositories[0].build_targets[0].diagnostic_status, "ready");

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_persists_repository_auth_state_in_inspection() {
        let root = std::env::temp_dir().join("desktop-shell-project-auth-state-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://github.com/indiegabo/workers.git")),
                local_path: None,
                repository_access_assessment: Some(RepositoryAccessAssessment {
                    provider_id: String::from("github"),
                    provider_label: String::from("GitHub"),
                    instance_url: String::from("https://github.com"),
                    normalized_url: String::from(
                        "https://github.com/indiegabo/workers.git",
                    ),
                    visibility: String::from("private"),
                    auth_requirement: String::from("required"),
                    auth_status: String::from("required_unbound"),
                    supports_interactive_login: true,
                    message: String::from(
                        "GitHub repository requires authentication before HGP can access it.",
                    ),
                }),
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path,
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect created project");

        assert_eq!(inspection.repositories.len(), 1);
        let repository = &inspection.repositories[0];
        assert_eq!(repository.repository_id, created.repository_id);
        assert_eq!(repository.source_provider_id.as_deref(), Some("github"));
        assert_eq!(
            repository.source_instance_url.as_deref(),
            Some("https://github.com")
        );
        assert_eq!(repository.visibility_status, "private");
        assert_eq!(repository.auth_requirement_status, "required");
        assert_eq!(repository.auth_binding_status, "required_unbound");
        assert!(repository
            .auth_status_message
            .contains("requires authentication"));
        assert!(repository.auth_last_verified_at.is_some());

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_does_not_bind_default_github_auth_credential() {
        let root = std::env::temp_dir().join("desktop-shell-project-github-auth-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "GitHub.com",
                    "git-http-github-host-login",
                    r#"{"provider":"github","instance_url":"https://github.com","credential_helper":"manager","auth_mode":"browser"}"#,
                ],
            )
            .expect("GitHub auth credential should insert");
        let github_credentials_id = connection.last_insert_rowid();
        drop(connection);

        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://github.com/indiegabo/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist");

        assert_eq!(created.credentials_id, None);

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect created project");

        assert_eq!(inspection.repositories.len(), 1);
        let repository = &inspection.repositories[0];
        assert!(
            repository.credentials.is_none(),
            "GitHub auth credential should remain unbound until the project explicitly connects it"
        );

        let settings = load_secret_settings(&config)
            .expect("secret settings should still expose the reusable credential record");
        assert_eq!(settings.credentials.len(), 1);
        assert_eq!(settings.credentials[0].credential_id, github_credentials_id);
        assert_eq!(settings.repository_bindings[0].credentials_id, None);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn finalize_github_auth_login_does_not_bind_matching_repositories() {
        let root = std::env::temp_dir().join("desktop-shell-github-auth-finalize-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params!["workers", "https://github.com/indiegabo/workers.git", "unity"],
            )
            .expect("repository should insert");
        drop(connection);

        let provider = finalize_github_auth_login_with_known_accounts(
            &storage,
            &[String::from("indiegabo")],
        )
            .expect("finalizing GitHub auth should persist the reusable credential record");

        assert_eq!(provider.status, AUTH_PROVIDER_STATUS_CONNECTED);
        assert_eq!(provider.bound_repository_count, 0);
        assert!(provider.credential_created_at.is_some());
        assert!(provider.credential_updated_at.is_some());

        let credential = resolve_github_auth_credential(&storage)
            .expect("GitHub auth credential lookup should succeed")
            .expect("finalizing GitHub auth should persist the reusable credential record");
        let config_json = serde_json::from_str::<serde_json::Value>(&credential.config_json)
            .expect("stored GitHub auth config should be valid JSON");
        assert_eq!(config_json.get("login").and_then(serde_json::Value::as_str), Some("indiegabo"));

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should keep the project unbound");
        assert_eq!(inspection.repositories.len(), 1);
        assert!(inspection.repositories[0].credentials.is_none());

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn select_github_auth_login_prefers_existing_known_login() {
        let selected = select_github_auth_login(
            Some(String::from("indiegabo")),
            &[
                String::from("x-access-token"),
                String::from("indiegabo"),
            ],
        );

        assert_eq!(selected.as_deref(), Some("indiegabo"));
    }

    #[test]
    fn select_github_auth_login_skips_placeholder_and_numeric_accounts() {
        let selected = select_github_auth_login(
            None,
            &[
                String::from("95456621"),
                String::from("x-oauth-basic"),
                String::from("x-access-token"),
                String::from("indiegabo"),
            ],
        );

        assert_eq!(selected.as_deref(), Some("indiegabo"));
    }

    #[test]
    fn github_auth_credential_config_json_includes_selected_login() {
        let config_json = github_auth_credential_config_json(Some("indiegabo"));
        let config_json = serde_json::from_str::<serde_json::Value>(&config_json)
            .expect("GitHub auth config should be valid JSON");

        assert_eq!(config_json.get("login").and_then(serde_json::Value::as_str), Some("indiegabo"));
    }

    #[test]
    fn github_browser_login_command_args_enable_force_reauthentication_when_requested() {
        let args = github_browser_login_command_args(true);

        assert!(args.contains(&"--force"));
        assert_eq!(args[0], "github");
        assert_eq!(args[1], "login");
    }

    #[test]
    fn github_browser_login_command_args_skip_force_by_default() {
        let args = github_browser_login_command_args(false);

        assert!(!args.contains(&"--force"));
        assert_eq!(args[0], "github");
        assert_eq!(args[1], "login");
    }

    #[test]
    fn persist_repository_project_binds_explicit_repository_credential() {
        let root = std::env::temp_dir().join("desktop-shell-project-explicit-auth-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "GitHub.com",
                    "git-http-github-host-login",
                    r#"{"provider":"github","instance_url":"https://github.com","credential_helper":"manager","auth_mode":"browser"}"#,
                ],
            )
            .expect("GitHub auth credential should insert");
        let github_credentials_id = connection.last_insert_rowid();
        drop(connection);

        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://github.com/indiegabo/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: Some(github_credentials_id),
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist with explicit auth binding");

        assert_eq!(created.credentials_id, Some(github_credentials_id));

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect explicit binding");
        assert_eq!(inspection.repositories.len(), 1);
        let credentials = inspection.repositories[0]
            .credentials
            .as_ref()
            .expect("explicit credential binding should be visible in inspection");
        assert_eq!(credentials.credential_id, github_credentials_id);

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_rejects_personal_access_tokens() {
        let root = std::env::temp_dir().join("desktop-shell-project-pat-rejection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let error = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://github.com/indiegabo/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: Some(String::from("ghp-legacy-token")),
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect_err("PAT-based repository auth should be rejected");

        assert!(
            error
                .to_string()
                .contains("personal access token is no longer supported")
        );

        if root.exists() {
            std::fs::remove_dir_all(root).expect("temp directory should be removable");
        }
    }

    #[test]
    fn persist_repository_project_rejects_unsupported_engine_kind() {
        let root = std::env::temp_dir().join("desktop-shell-project-engine-rejection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let error = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unreal"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://example.com/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path,
                }],
                publish_targets: vec![],
            },
        )
        .expect_err("non-Unity repository engines should be rejected");

        assert!(
            error
                .to_string()
                .contains("only \"unity\" is currently allowed")
        );

        if root.exists() {
            std::fs::remove_dir_all(root).expect("temp directory should be removable");
        }
    }

    #[test]
    fn persist_repository_project_update_refreshes_repository_inspection_entry() {
        let root = std::env::temp_dir().join("desktop-shell-project-update-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://example.com/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: Some(String::from("C:/artifacts/workers")),
                workspace_root_override: Some(String::from("C:/workspaces/workers")),
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist");

        persist_repository_project_update(
            &config,
            UpdateRepositoryProjectCommandInput {
                repository_id: created.repository_id,
                name: String::from("Workers Updated"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repository_url: Some(String::from("https://example.com/workers-updated.git")),
                local_path: None,
                repository_access_assessment: None,
                default_branch: Some(String::from("release")),
                artifacts_root_override: None,
                workspace_root_override: Some(String::from("D:/workspaces/workers")),
                polling_interval_seconds: 45,
                enabled: false,
                build_targets: vec![
                    UpdateRepositoryProjectBuildTargetCommandInput {
                        build_target_id: Some(created.build_target_ids[0]),
                        name: String::from("Windows Stable"),
                        contract: BuildContractCommandInput {
                            unity: Some(UnityBuildContractCommandInput {
                                target_platform: String::from("StandaloneWindows64"),
                                build_method: String::from("Builder.PerformWindowsStable"),
                            }),
                        },
                        unity_executable_path: unity_executable_path.clone(),
                    },
                    UpdateRepositoryProjectBuildTargetCommandInput {
                        build_target_id: None,
                        name: String::from("WebGL"),
                        contract: BuildContractCommandInput {
                            unity: Some(UnityBuildContractCommandInput {
                                target_platform: String::from("WebGL"),
                                build_method: String::from("Builder.PerformWebGl"),
                            }),
                        },
                        unity_executable_path,
                    },
                ],
                publish_targets: vec![],
            },
        )
        .expect("repository project update should persist");

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect updated project");

        assert_eq!(inspection.repositories.len(), 1);
        assert_eq!(inspection.repositories[0].repository_id, created.repository_id);
        assert_eq!(inspection.repositories[0].repository_name, "Workers Updated");
        assert_eq!(
            inspection.repositories[0].repo_url,
            "https://example.com/workers-updated.git"
        );
        assert_eq!(inspection.repositories[0].default_branch.as_deref(), Some("release"));
        assert_eq!(inspection.repositories[0].artifacts_root_override, None);
        assert_eq!(
            inspection.repositories[0].workspace_root_override.as_deref(),
            Some("D:/workspaces/workers")
        );
        assert_eq!(inspection.repositories[0].polling_interval_seconds, 45);
        assert!(!inspection.repositories[0].enabled);
        assert_eq!(inspection.repositories[0].enabled_build_target_count, 2);
        assert_eq!(inspection.repositories[0].build_targets.len(), 2);
        assert_eq!(inspection.repositories[0].build_targets[0].target_name, "Windows Stable");
        assert_eq!(inspection.repositories[0].build_targets[1].target_name, "WebGL");

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_update_rejects_unsupported_engine_kind() {
        let root = std::env::temp_dir().join("desktop-shell-project-update-engine-rejection-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let unity_executable_path = std::env::current_exe()
            .expect("current executable path should resolve")
            .display()
            .to_string();

        let created = persist_repository_project(
            &config,
            CreateRepositoryProjectCommandInput {
                name: String::from("Workers"),
                engine_kind: String::from("unity"),
                source_mode: Some(String::from("managed_repository")),
                repository_url: Some(String::from("https://example.com/workers.git")),
                local_path: None,
                repository_access_assessment: None,
                repository_credentials_id: None,
                personal_access_token: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                build_targets: vec![CreateRepositoryProjectBuildTargetCommandInput {
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path: unity_executable_path.clone(),
                }],
                publish_targets: vec![],
            },
        )
        .expect("repository project should persist");

        let error = persist_repository_project_update(
            &config,
            UpdateRepositoryProjectCommandInput {
                repository_id: created.repository_id,
                name: String::from("Workers Updated"),
                engine_kind: String::from("unreal"),
                source_mode: String::from("managed_repository"),
                repository_url: Some(String::from("https://example.com/workers-updated.git")),
                local_path: None,
                repository_access_assessment: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![UpdateRepositoryProjectBuildTargetCommandInput {
                    build_target_id: Some(created.build_target_ids[0]),
                    name: String::from("Windows"),
                    contract: BuildContractCommandInput {
                        unity: Some(UnityBuildContractCommandInput {
                            target_platform: String::from("StandaloneWindows64"),
                            build_method: String::from("Builder.PerformWindows"),
                        }),
                    },
                    unity_executable_path,
                }],
                publish_targets: vec![],
            },
        )
        .expect_err("non-Unity repository engines should be rejected on update");

        assert!(
            error
                .to_string()
                .contains("only \"unity\" is currently allowed")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_removal_detaches_project_and_keeps_runtime_files() {
        let root = std::env::temp_dir().join("desktop-shell-project-remove-detach-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");
        let fixture = seed_repository_project_removal_fixture(&storage, &root);

        let report = persist_repository_project_removal(
            &config,
            RemoveRepositoryProjectCommandInput {
                repository_id: fixture.repository_id,
                strategy: RemoveRepositoryProjectStrategy::Detach,
            },
        )
        .expect("repository detach should succeed");

        assert_eq!(report.repository_id, fixture.repository_id);
        assert_eq!(report.strategy, RemoveRepositoryProjectStrategy::Detach);
        assert!(report.removed_paths.is_empty());
        assert!(report.missing_paths.is_empty());
        assert!(report.skipped_paths.is_empty());
        assert!(fixture.workspace_path.exists());
        assert!(fixture.artifact_root_path.exists());
        assert!(fixture.build_log_path.exists());
        assert!(fixture.stage_log_path.exists());
        assert!(fixture.retained_file_path.exists());

        let connection = open_connection(&storage.database_path).expect("connection should open");
        let repository_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM repositories WHERE id = ?",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("repository count should load");
        let release_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM release_runs WHERE repository_id = ?",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("release count should load");
        let build_count: i64 = connection
            .query_row(
                "
                SELECT COUNT(1)
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE rr.repository_id = ?
                ",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("build count should load");
        let publish_count: i64 = connection
            .query_row(
                "
                SELECT COUNT(1)
                FROM publish_runs pr
                JOIN release_runs rr ON rr.id = pr.release_run_id
                WHERE rr.repository_id = ?
                ",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("publish count should load");
        let queue_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_queue_messages",
                [],
                |row| row.get(0),
            )
            .expect("queue count should load");
        let lease_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_coordination_leases",
                [],
                |row| row.get(0),
            )
            .expect("lease count should load");
        let idempotency_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_idempotency_keys",
                [],
                |row| row.get(0),
            )
            .expect("idempotency count should load");

        assert_eq!(repository_count, 0);
        assert_eq!(release_count, 0);
        assert_eq!(build_count, 0);
        assert_eq!(publish_count, 0);
        assert_eq!(queue_count, 0);
        assert_eq!(lease_count, 0);
        assert_eq!(idempotency_count, 0);

        drop(connection);
        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    #[test]
    fn persist_repository_project_removal_purges_runtime_files() {
        let root = std::env::temp_dir().join("desktop-shell-project-remove-purge-test");
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("existing temp directory should be removable");
        }

        let config = RuntimeConfig::from_root(&root);
        let storage = StorageLayout::from_directories(&config.directories);
        initialize_database(&storage).expect("database bootstrap should succeed");
        let fixture = seed_repository_project_removal_fixture(&storage, &root);

        let report = persist_repository_project_removal(
            &config,
            RemoveRepositoryProjectCommandInput {
                repository_id: fixture.repository_id,
                strategy: RemoveRepositoryProjectStrategy::Purge,
            },
        )
        .expect("repository purge should succeed");

        assert_eq!(report.repository_id, fixture.repository_id);
        assert_eq!(report.strategy, RemoveRepositoryProjectStrategy::Purge);
        assert!(report.missing_paths.is_empty());
        assert!(report.skipped_paths.is_empty());
        assert!(report.removed_paths.contains(&fixture.workspace_path));
        assert!(report.removed_paths.contains(&fixture.artifact_root_path));
        assert!(report.removed_paths.contains(&fixture.build_log_path));
        assert!(report.removed_paths.contains(&fixture.stage_log_path));
        assert!(report.removed_paths.contains(&fixture.retained_file_path));
        assert!(!fixture.workspace_path.exists());
        assert!(!fixture.artifact_root_path.exists());
        assert!(!fixture.build_log_path.exists());
        assert!(!fixture.stage_log_path.exists());
        assert!(!fixture.retained_file_path.exists());

        let connection = open_connection(&storage.database_path).expect("connection should open");
        let repository_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM repositories WHERE id = ?",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("repository count should load");
        let queue_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_queue_messages",
                [],
                |row| row.get(0),
            )
            .expect("queue count should load");
        let lease_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_coordination_leases",
                [],
                |row| row.get(0),
            )
            .expect("lease count should load");
        let idempotency_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_idempotency_keys",
                [],
                |row| row.get(0),
            )
            .expect("idempotency count should load");

        assert_eq!(repository_count, 0);
        assert_eq!(queue_count, 0);
        assert_eq!(lease_count, 0);
        assert_eq!(idempotency_count, 0);

        drop(connection);
        std::fs::remove_dir_all(root).expect("temp directory should be removable");
    }

    struct RepositoryProjectRemovalFixture {
        repository_id: i64,
        workspace_path: PathBuf,
        artifact_root_path: PathBuf,
        build_log_path: PathBuf,
        stage_log_path: PathBuf,
        retained_file_path: PathBuf,
    }

    fn seed_repository_project_removal_fixture(
        storage: &StorageLayout,
        root: &Path,
    ) -> RepositoryProjectRemovalFixture {
        let workspace_path = root.join("runtime-workspaces").join("release-run-91");
        let artifact_root_path = root.join("runtime-artifacts").join("build-run-91");
        let build_log_path = root.join("runtime-logs").join("build-run-91.log");
        let stage_log_path = root
            .join("runtime-logs")
            .join("steps")
            .join("build-run-91-stage-01.log");
        let retained_file_path = root
            .join("retained-files")
            .join("build-run-91-report.json");

        std::fs::create_dir_all(&workspace_path).expect("workspace directory should create");
        std::fs::create_dir_all(&artifact_root_path)
            .expect("artifact directory should create");
        std::fs::create_dir_all(build_log_path.parent().expect("log parent should exist"))
            .expect("log directory should create");
        std::fs::create_dir_all(stage_log_path.parent().expect("stage log parent should exist"))
            .expect("stage log directory should create");
        std::fs::create_dir_all(
            retained_file_path.parent().expect("retained parent should exist"),
        )
        .expect("retained directory should create");
        std::fs::write(workspace_path.join("README.txt"), "workspace")
            .expect("workspace file should write");
        std::fs::write(artifact_root_path.join("game.zip"), "artifact")
            .expect("artifact file should write");
        std::fs::write(&build_log_path, "build log")
            .expect("build log should write");
        std::fs::write(&stage_log_path, "stage log")
            .expect("stage log should write");
        std::fs::write(&retained_file_path, "{\"status\":\"retained\"}")
            .expect("retained file should write");

        let connection = open_connection(&storage.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params![
                    "removal-repo",
                    "https://example.com/removal-repo.git",
                    "unity"
                ],
            )
            .expect("repository should insert");
        let repository_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    contract_json,
                    config_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    r#"{"unity":{"targetPlatform":"StandaloneWindows64","buildMethod":"CI.Build.Perform"}}"#,
                    "{}",
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, "filesystem-release", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO release_runs (
                    repository_id,
                    git_tag,
                    git_commit,
                    engine_version,
                    status
                )
                VALUES (?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "v1.9.1",
                    "deadbeef",
                    "2022.3.20f1",
                    "queued",
                ],
            )
            .expect("release run should insert");
        let release_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO build_runs (
                    release_run_id,
                    build_target_id,
                    engine_version,
                    image_ref,
                    status,
                    workspace_path,
                    log_path,
                    artifact_root_path
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    "2022.3.20f1",
                    "host-native",
                    "queued",
                    workspace_path.display().to_string(),
                    build_log_path.display().to_string(),
                    artifact_root_path.display().to_string(),
                ],
            )
            .expect("build run should insert");
        let build_run_id = connection.last_insert_rowid();
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
                    last_message
                )
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    1,
                    "checkout-repository",
                    "Checkout Repository",
                    "succeeded",
                    stage_log_path.display().to_string(),
                    "checked out source",
                ],
            )
            .expect("build stage should insert");
        connection
            .execute(
                "
                INSERT INTO artifacts (
                    build_run_id,
                    name,
                    kind,
                    path,
                    active_location_kind,
                    active_location_ref
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    "game.zip",
                    "archive",
                    "game.zip",
                    "runtime_artifact",
                    artifact_root_path.join("game.zip").display().to_string(),
                ],
            )
            .expect("artifact should insert");
        let artifact_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status,
                    execution_contract_json
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    "queued",
                    "{}",
                ],
            )
            .expect("publish run should insert");
        let publish_run_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                INSERT INTO retained_execution_files (
                    build_run_id,
                    role,
                    path,
                    status
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    "execution_report",
                    retained_file_path.display().to_string(),
                    "retained",
                ],
            )
            .expect("retained execution file should insert");
        connection
            .execute(
                "
                INSERT INTO worker_queue_messages (queue_name, payload)
                VALUES (?, ?)
                ",
                params![
                    "release-runs",
                    format!(
                        "{{\"release_run_id\":{release_run_id},\"repository_id\":{repository_id},\"git_tag\":\"v1.9.1\",\"git_commit\":\"deadbeef\",\"trigger_source\":\"manual\",\"trigger_rule_id\":null}}"
                    )
                    .into_bytes(),
                ],
            )
            .expect("release queue message should insert");
        connection
            .execute(
                "
                INSERT INTO worker_queue_messages (queue_name, payload)
                VALUES (?, ?)
                ",
                params![
                    "build-runs",
                    format!(
                        "{{\"build_run_id\":{build_run_id},\"release_run_id\":{release_run_id},\"build_target_id\":{build_target_id},\"engine_version\":\"2022.3.20f1\",\"image_ref\":\"host-native\"}}"
                    )
                    .into_bytes(),
                ],
            )
            .expect("build queue message should insert");
        connection
            .execute(
                "
                INSERT INTO worker_queue_messages (queue_name, payload)
                VALUES (?, ?)
                ",
                params![
                    "publish-runs",
                    format!(
                        "{{\"publish_run_id\":{publish_run_id},\"release_run_id\":{release_run_id},\"build_run_id\":{build_run_id},\"publish_target_id\":{publish_target_id},\"artifact_id\":{artifact_id}}}"
                    )
                    .into_bytes(),
                ],
            )
            .expect("publish queue message should insert");
        connection
            .execute(
                "
                INSERT INTO worker_coordination_leases (
                    name,
                    token,
                    lease_expires_at_unix_millis
                ) VALUES (?, ?, ?)
                ",
                params![format!("release-plan:{release_run_id}"), "lock-a", 9_999_999_999_i64],
            )
            .expect("release coordination lease should insert");
        connection
            .execute(
                "
                INSERT INTO worker_coordination_leases (
                    name,
                    token,
                    lease_expires_at_unix_millis
                ) VALUES (?, ?, ?)
                ",
                params![
                    format!("build-run:{build_run_id}:dispatch"),
                    "lock-b",
                    9_999_999_999_i64,
                ],
            )
            .expect("build coordination lease should insert");
        connection
            .execute(
                "
                INSERT INTO worker_coordination_leases (
                    name,
                    token,
                    lease_expires_at_unix_millis
                ) VALUES (?, ?, ?)
                ",
                params![
                    format!("publish-run:{publish_run_id}:dispatch"),
                    "lock-c",
                    9_999_999_999_i64,
                ],
            )
            .expect("publish coordination lease should insert");
        connection
            .execute(
                "
                INSERT INTO worker_idempotency_keys (
                    idempotency_key,
                    claim_expires_at_unix_millis
                ) VALUES (?, ?)
                ",
                params![
                    format!("release-run:{release_run_id}:queued"),
                    9_999_999_999_i64,
                ],
            )
            .expect("release idempotency key should insert");
        connection
            .execute(
                "
                INSERT INTO worker_idempotency_keys (
                    idempotency_key,
                    claim_expires_at_unix_millis
                ) VALUES (?, ?)
                ",
                params![
                    format!("build-run:{build_run_id}:queued"),
                    9_999_999_999_i64,
                ],
            )
            .expect("build idempotency key should insert");
        connection
            .execute(
                "
                INSERT INTO worker_idempotency_keys (
                    idempotency_key,
                    claim_expires_at_unix_millis
                ) VALUES (?, ?)
                ",
                params![
                    format!("publish-run:{publish_run_id}:queued"),
                    9_999_999_999_i64,
                ],
            )
            .expect("publish idempotency key should insert");
        drop(connection);

        RepositoryProjectRemovalFixture {
            repository_id,
            workspace_path,
            artifact_root_path,
            build_log_path,
            stage_log_path,
            retained_file_path,
        }
    }
}