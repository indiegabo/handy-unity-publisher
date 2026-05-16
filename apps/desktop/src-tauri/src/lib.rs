//! Implements the Tauri desktop shell bindings that supervise the bundled
//! runtime and expose operator-facing diagnostics to the UI.

mod runtime_events;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use rfd::FileDialog;
use runtime_config::{
    HostPlatform, RuntimeConfig, RUNTIME_ROOT_ENV,
};
use runtime_core::{
    read_health_report, read_supervision_contract, read_supervisor_snapshot,
    RuntimeHealthReport, RuntimeRestartPolicy, RuntimeSupervisorSnapshot,
    RuntimeStatus, RuntimeSupervisorStatus,
};
use runtime_git::{
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
    CreateRepositoryProjectInput as StoreCreateRepositoryProjectInput,
    CreatedRepositoryProjectRecord,
    UpdateRepositoryProjectBuildTargetInput as StoreUpdateRepositoryProjectBuildTargetInput,
    UpdateRepositoryProjectInput as StoreUpdateRepositoryProjectInput,
    RuntimeControlRequest,
    ProcessFeedPage, ReleaseAutomationStatus, UpsertCredentialRecordInput,
    enqueue_runtime_control_request,
    initialize_database, list_artifact_inspection_records,
    list_build_history_records, list_process_feed_page,
    list_build_target_runtime_settings, list_credential_records,
    list_publish_target_runtime_settings, LocalCoordinator, StorageLayout,
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

const RUNTIME_PACKAGE_NAME: &str = "runtime-bin";
const RUNTIME_BINARY_NAME: &str = "hgp-runtime";
const DEFAULT_RUNTIME_LOG_LINE_LIMIT: usize = 100;
const MAX_RUNTIME_LOG_LINE_LIMIT: usize = 500;
const SECRET_STORAGE_MODEL_INLINE_SQLITE: &str =
    "sqlite-config-json-and-keyring-references";
const RUNTIME_STARTUP_PROBE_MILLIS: u64 = 150;
const RUNTIME_SHUTDOWN_WAIT_POLL_MILLIS: u64 = 100;
const RUNTIME_SHUTDOWN_WAIT_POLLS: usize = 20;
const BUILD_EXECUTION_RETAINED_DIR_NAME: &str = "retained";
const BUILD_EXECUTION_REPORT_FILE_NAME: &str = "execution-report.json";
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
const DEFAULT_PROCESS_FEED_PAGE_SIZE: u32 = 6;
const MAX_PROCESS_FEED_PAGE_SIZE: u32 = 50;
const DEFAULT_HOST_NATIVE_RUNNER_TYPE: &str = "host-native";
const DEFAULT_BUILD_TARGET_TIMEOUT_SECONDS: i64 = 3600;
const MIN_REPOSITORY_POLL_INTERVAL_SECONDS: i64 = 5;
const SUPPORTED_REPOSITORY_ENGINE_KIND_UNITY: &str = "unity";
const GITHUB_AUTH_PROVIDER_ID: &str = "github";
const GITHUB_AUTH_PROVIDER_LABEL: &str = "GitHub";
const GITHUB_AUTH_INSTANCE_URL: &str = "https://github.com";
const GITHUB_AUTH_CREDENTIAL_NAME: &str = "GitHub.com";
const GITHUB_AUTH_CREDENTIAL_HELPER: &str = "manager";
const GITHUB_AUTH_MODE_BROWSER: &str = "browser";
const AUTH_PROVIDER_STATUS_CONNECTED: &str = "connected";
const AUTH_PROVIDER_STATUS_DISCONNECTED: &str = "disconnected";
const AUTH_PROVIDER_STATUS_UNAVAILABLE: &str = "unavailable";

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
struct UpdateRepositorySecretBindingInput {
    repository_id: i64,
    credentials_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdatePublishTargetSecretBindingInput {
    publish_target_id: i64,
    credentials_id: Option<i64>,
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
    repository_url: String,
    personal_access_token: Option<String>,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    build_targets: Vec<CreateRepositoryProjectBuildTargetCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectBuildTargetCommandInput {
    build_target_id: Option<i64>,
    name: String,
    contract: BuildContractCommandInput,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct UpdateRepositoryProjectCommandInput {
    repository_id: i64,
    name: String,
    engine_kind: String,
    repository_url: String,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    enabled: bool,
    build_targets: Vec<UpdateRepositoryProjectBuildTargetCommandInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct RepositoryInstantCheckInput {
    repository_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectBuildTargetCommandInput {
    name: String,
    target_platform: String,
    build_method: String,
    unity_executable_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedCreateRepositoryProjectCommandInput {
    name: String,
    engine_kind: String,
    repository_url: String,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    build_targets: Vec<NormalizedCreateRepositoryProjectBuildTargetCommandInput>,
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
struct NormalizedUpdateRepositoryProjectCommandInput {
    repository_id: i64,
    name: String,
    engine_kind: String,
    repository_url: String,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    polling_interval_seconds: i64,
    enabled: bool,
    build_targets: Vec<NormalizedUpdateRepositoryProjectBuildTargetCommandInput>,
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
struct RepositoryPublishTargetInspection {
    publish_target_id: i64,
    name: String,
    kind: String,
    enabled: bool,
    credentials: Option<RepositoryCredentialReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RepositoryInspectionEntry {
    repository_id: i64,
    repository_name: String,
    repo_url: String,
    engine_kind: String,
    enabled: bool,
    polling_interval_seconds: i64,
    default_branch: Option<String>,
    artifacts_root_override: Option<String>,
    workspace_root_override: Option<String>,
    last_seen_tag: Option<String>,
    enabled_build_target_count: i64,
    credentials: Option<RepositoryCredentialReference>,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
struct BuildExecutionReportPayload {
    build_run_id: i64,
    workspace_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    exists: bool,
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
struct ApplicationVersionInfo {
    product_name: String,
    app_version: String,
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
    let has_active_system_dialogs = lifecycle
        .active_system_dialogs
        .lock()
        .map(|active_system_dialogs| *active_system_dialogs > 0)
        .unwrap_or(true);

    !is_pinned && !has_active_system_dialogs
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
            transition_window_focus,
            main_window_pin_state,
            set_main_window_pinned,
            close_main_window,
            pick_host_path,
            pick_unity_executable_path,
            validate_unity_executable_path,
            create_repository_project,
            update_repository_project,
            runtime_health,
            runtime_logs,
            runtime_directories,
            runtime_lifecycle_settings,
            release_status,
            repository_inspection,
            build_history,
            artifact_inspection,
            build_execution_report,
            purge_build_execution_retention,
            auth_providers,
            login_github_auth,
            secret_settings,
            save_secret_credential,
            update_repository_secret_binding,
            update_publish_target_secret_binding,
            start_runtime,
            stop_runtime,
            restart_runtime,
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
    let _dialog_guard = ActiveSystemDialogGuard::acquire(&lifecycle)?;

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
fn runtime_health() -> Result<RuntimeHealthReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_health_report(&config).map_err(|error| error.to_string())
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
fn runtime_lifecycle_settings() -> Result<RuntimeLifecycleSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_runtime_lifecycle_settings(&config).map_err(|error| error.to_string())
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
fn purge_build_execution_retention(
    build_run_id: i64,
) -> Result<BuildExecutionRetentionPurgeReport, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    purge_build_execution_retention_files(&config, build_run_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn auth_providers() -> Result<Vec<AuthProviderStatus>, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_auth_providers(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn login_github_auth() -> Result<AuthProviderStatus, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_github_auth_login(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn secret_settings() -> Result<SecretSettings, String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    load_secret_settings(&config).map_err(|error| error.to_string())
}

#[tauri::command]
fn save_secret_credential(input: SaveSecretCredentialInput) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_secret_credential(&config, input).map_err(|error| error.to_string())
}

#[tauri::command]
fn update_repository_secret_binding(
    input: UpdateRepositorySecretBindingInput,
) -> Result<(), String> {
    let config = load_shell_runtime_config().map_err(|error| error.to_string())?;
    persist_repository_secret_binding(&config, input)
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
    command.env(RUNTIME_ROOT_ENV, &config.directories.data_dir);
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
    command.env(RUNTIME_ROOT_ENV, &config.directories.data_dir);
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

    let release_status = load_release_status(config)?;
    let unity_adapter_settings = load_unity_adapter_settings(config)?;
    let secret_settings = load_secret_settings(config)?;
    let generated_at = release_status.generated_at.clone();
    let release_status_by_repository = release_status
        .repositories
        .into_iter()
        .map(|repository| (repository.repository_id, repository))
        .collect::<HashMap<_, _>>();
    let credential_by_id = secret_settings
        .credentials
        .into_iter()
        .map(|credential| {
            (
                credential.credential_id,
                RepositoryCredentialReference {
                    credential_id: credential.credential_id,
                    name: credential.name,
                    kind: credential.kind,
                    config_status: credential.config_summary.status,
                    config_message: credential.config_summary.message,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    let mut build_targets_by_repository = HashMap::<
        i64,
        Vec<UnityAdapterBuildTargetSettings>,
    >::new();
    for target in unity_adapter_settings.build_targets {
        build_targets_by_repository
            .entry(target.repository_id)
            .or_default()
            .push(target);
    }

    let mut publish_targets_by_repository =
        HashMap::<i64, Vec<RepositoryPublishTargetInspection>>::new();
    for target in secret_settings.publish_target_bindings {
        publish_targets_by_repository
            .entry(target.repository_id)
            .or_default()
            .push(RepositoryPublishTargetInspection {
                publish_target_id: target.publish_target_id,
                name: target.publish_target_name,
                kind: target.publish_target_kind,
                enabled: target.enabled,
                credentials: clone_credential_reference(
                    &credential_by_id,
                    target.credentials_id,
                ),
            });
    }

    let repositories = LocalCoordinator::new(&storage)
        .list_polling_repositories()?
        .into_iter()
        .map(|repository| {
            let release_status = release_status_by_repository.get(&repository.id);

            RepositoryInspectionEntry {
                repository_id: repository.id,
                repository_name: repository.name,
                repo_url: repository.repo_url,
                engine_kind: repository.engine_kind,
                enabled: repository.enabled,
                polling_interval_seconds: repository.polling_interval_seconds,
                default_branch: repository.default_branch,
                artifacts_root_override: repository.artifacts_root_override,
                workspace_root_override: repository.workspace_root_override,
                last_seen_tag: repository.last_seen_tag,
                enabled_build_target_count: repository.enabled_build_target_count,
                credentials: clone_credential_reference(
                    &credential_by_id,
                    repository.credentials_id,
                ),
                build_targets: build_targets_by_repository
                    .remove(&repository.id)
                    .unwrap_or_default(),
                publish_targets: publish_targets_by_repository
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
        })
        .collect();

    Ok(RepositoryInspectionSettings {
        generated_at,
        repositories,
    })
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
            report_path: None,
            exists: false,
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
    let report_path = workspace_path.as_deref().map(build_execution_report_path);
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
        report_path,
        exists: report.is_some(),
        report,
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

    let credential = ensure_github_auth_credential(&storage)?;
    let bound_repository_count = count_repository_bindings(&storage, credential.id)?;

    Ok(build_auth_provider_status(
        AUTH_PROVIDER_STATUS_CONNECTED,
        format!(
            "Git Credential Manager has an active GitHub login. {bound_repository_count} repository project(s) currently use it by default."
        ),
        Some(&credential),
        bound_repository_count,
    ))
}

fn persist_github_auth_login(config: &RuntimeConfig) -> io::Result<AuthProviderStatus> {
    ensure_git_credential_manager_available()?;
    run_github_browser_login_command()?;

    let storage = writable_secret_storage(config)?;
    let credential = ensure_github_auth_credential(&storage)?;
    let _ = bind_github_auth_to_repositories(&storage, credential.id)?;
    let bound_repository_count = count_repository_bindings(&storage, credential.id)?;

    Ok(build_auth_provider_status(
        AUTH_PROVIDER_STATUS_CONNECTED,
        format!(
            "GitHub login connected through Git Credential Manager. {bound_repository_count} repository project(s) now use it by default."
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

fn run_github_browser_login_command() -> io::Result<()> {
    let _ = run_git_credential_manager_command([
        "github",
        "login",
        "--browser",
        "--url",
        GITHUB_AUTH_INSTANCE_URL,
    ])?;

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

fn ensure_github_auth_credential(storage: &StorageLayout) -> io::Result<CredentialRecord> {
    let existing = resolve_github_auth_credential(storage)?;
    LocalCoordinator::new(storage).upsert_credential_record(
        UpsertCredentialRecordInput {
            credential_id: existing.as_ref().map(|credential| credential.id),
            name: String::from(GITHUB_AUTH_CREDENTIAL_NAME),
            kind: String::from(KIND_GIT_HTTP_GITHUB_HOST_LOGIN),
            config_json: github_auth_credential_config_json(),
        },
    )
}

fn github_auth_credential_config_json() -> String {
    serde_json::json!({
        "provider": GITHUB_AUTH_PROVIDER_ID,
        "instance_url": GITHUB_AUTH_INSTANCE_URL,
        "credential_helper": GITHUB_AUTH_CREDENTIAL_HELPER,
        "auth_mode": GITHUB_AUTH_MODE_BROWSER,
    })
    .to_string()
}

fn bind_github_auth_to_repositories(
    storage: &StorageLayout,
    credential_id: i64,
) -> io::Result<usize> {
    let coordinator = LocalCoordinator::new(storage);
    let repositories = coordinator.list_polling_repositories()?;
    let mut updated_bindings = 0;

    for repository in repositories {
        if !is_github_repository_url(&repository.repo_url) {
            continue;
        }
        if repository.credentials_id == Some(credential_id) {
            continue;
        }

        coordinator.update_repository_credentials_binding(
            repository.id,
            Some(credential_id),
        )?;
        updated_bindings += 1;
    }

    Ok(updated_bindings)
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

fn resolve_default_github_auth_credential(
    storage: &StorageLayout,
    repository_url: &str,
) -> io::Result<Option<CredentialRecord>> {
    if !is_github_repository_url(repository_url) {
        return Ok(None);
    }

    if let Some(credential) = resolve_github_auth_credential(storage)? {
        return Ok(Some(credential));
    }

    if !git_credential_manager_available() {
        return Ok(None);
    }

    let known_accounts = match load_known_github_accounts() {
        Ok(accounts) => accounts,
        Err(_) => return Ok(None),
    };
    if known_accounts.is_empty() {
        return Ok(None);
    }

    ensure_github_auth_credential(storage).map(Some)
}

fn is_github_repository_url(repository_url: &str) -> bool {
    github_repository_owner_from_url(repository_url).is_some()
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
) -> io::Result<()> {
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
    LocalCoordinator::new(&storage).upsert_credential_record(
        UpsertCredentialRecordInput {
            credential_id: input.credential_id,
            name: input.name,
            kind: input.kind,
            config_json: input.config_json,
        },
    )?;

    Ok(())
}

fn persist_repository_secret_binding(
    config: &RuntimeConfig,
    input: UpdateRepositorySecretBindingInput,
) -> io::Result<()> {
    let storage = writable_secret_storage(config)?;
    LocalCoordinator::new(&storage).update_repository_credentials_binding(
        input.repository_id,
        input.credentials_id,
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
    let default_github_auth = resolve_default_github_auth_credential(
        &storage,
        &normalized.repository_url,
    )?;

    let mut create_result = LocalCoordinator::new(&storage).create_repository_project(
        StoreCreateRepositoryProjectInput {
            name: normalized.name,
            engine_kind: normalized.engine_kind,
            repo_url: normalized.repository_url,
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
        },
    )?;

    if let Some(credential) = default_github_auth {
        LocalCoordinator::new(&storage).update_repository_credentials_binding(
            create_result.repository_id,
            Some(credential.id),
        )?;
        create_result.credentials_id = Some(credential.id);
    }

    Ok(create_result)
}

fn persist_repository_project_update(
    config: &RuntimeConfig,
    input: UpdateRepositoryProjectCommandInput,
) -> io::Result<()> {
    let normalized = normalize_update_repository_project_command_input(input)?;
    let storage = writable_secret_storage(config)?;

    LocalCoordinator::new(&storage).update_repository_project(
        StoreUpdateRepositoryProjectInput {
            repository_id: normalized.repository_id,
            name: normalized.name,
            engine_kind: normalized.engine_kind,
            repo_url: normalized.repository_url,
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
        },
    )
}

fn normalize_create_repository_project_command_input(
    input: CreateRepositoryProjectCommandInput,
) -> io::Result<NormalizedCreateRepositoryProjectCommandInput> {
    let engine_kind = normalize_repository_project_engine_kind(&input.engine_kind)?;
    let name = require_shell_non_empty(&input.name, "repository project name")?;
    let repository_url = require_shell_non_empty(
        &input.repository_url,
        "repository project URL",
    )?;
    if !(repository_url.starts_with("https://") || repository_url.starts_with("http://")) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository project URL must use http:// or https://",
        ));
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

    Ok(NormalizedCreateRepositoryProjectCommandInput {
        name,
        engine_kind,
        repository_url,
        default_branch: normalize_optional_shell_string(input.default_branch),
        artifacts_root_override: normalize_optional_shell_string(input.artifacts_root_override),
        workspace_root_override: normalize_optional_shell_string(input.workspace_root_override),
        polling_interval_seconds: input.polling_interval_seconds,
        build_targets,
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
    let repository_url = require_shell_non_empty(
        &input.repository_url,
        "repository project URL",
    )?;
    if !(repository_url.starts_with("https://") || repository_url.starts_with("http://")) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "repository project URL must use http:// or https://",
        ));
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

    Ok(NormalizedUpdateRepositoryProjectCommandInput {
        repository_id: input.repository_id,
        name,
        engine_kind,
        repository_url,
        default_branch: normalize_optional_shell_string(input.default_branch),
        artifacts_root_override: normalize_optional_shell_string(input.artifacts_root_override),
        workspace_root_override: normalize_optional_shell_string(input.workspace_root_override),
        polling_interval_seconds: input.polling_interval_seconds,
        enabled: input.enabled,
        build_targets,
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

fn github_repository_owner_from_url(repository_url: &str) -> Option<String> {
    let repository_url = repository_url.trim();
    let without_scheme = repository_url
        .strip_prefix("https://")
        .or_else(|| repository_url.strip_prefix("http://"))?;
    let (host, path) = without_scheme.split_once('/')?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }

    let owner = path.split('/').next()?.trim();
    if owner.is_empty() {
        return None;
    }

    Some(owner.to_owned())
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
    ]
}

fn credential_kind_supported(kind: &str) -> bool {
    matches!(
        kind.trim(),
        KIND_GIT_HTTP_BASIC
            | KIND_GIT_HTTP_BEARER
            | KIND_GIT_HTTP_GITHUB_HOST_LOGIN
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
    let capability_profile = inspect_host_capability_profile(config.platform);
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
        Err(error) if error.kind() == ErrorKind::NotFound => {
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
        Err(error) if error.kind() == ErrorKind::NotFound => {}
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
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    pids.sort_unstable();
    pids.dedup();
    Ok(pids)
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

#[cfg(test)]
mod tests {
    use super::{
        load_artifact_inspection,
        load_build_execution_report,
        load_build_history,
        load_process_feed,
        load_repository_inspection,
        load_release_status,
        development_runtime_command_plan, load_runtime_directory_settings,
        load_runtime_health_report, load_runtime_lifecycle_settings,
        load_runtime_log_lines,
        persist_repository_project,
        persist_repository_project_update,
        persist_publish_target_secret_binding,
        persist_repository_secret_binding,
        persist_secret_credential,
        process_identity_matches_runtime,
        purge_build_execution_retention_files,
        load_secret_settings,
        load_unity_adapter_settings,
        normalize_runtime_log_line_limit, packaged_runtime_command_plan,
        runtime_binary_file_name, RuntimeLaunchAction, RUNTIME_BINARY_NAME,
        BuildContractCommandInput,
        CreateRepositoryProjectBuildTargetCommandInput,
        CreateRepositoryProjectCommandInput,
        ProcessFeedInput,
        UpdateRepositoryProjectBuildTargetCommandInput,
        UpdateRepositoryProjectCommandInput,
        SaveSecretCredentialInput, UpdatePublishTargetSecretBindingInput,
        UnityBuildContractCommandInput, UpdateRepositorySecretBindingInput,
        window_transition_settings,
    };
    use runtime_config::RuntimeConfig;
    use runtime_core::{
        bootstrap_runtime, write_supervisor_snapshot, RuntimeRestartPolicy,
        RuntimeStatus, RuntimeSupervisorSnapshot, RuntimeSupervisorStatus,
    };
    use runtime_store::{
        initialize_database, open_connection, LocalCoordinator,
        ManualReleaseDispatchInput, StorageLayout,
    };
    use rusqlite::params;
    use std::path::{Path, PathBuf};

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
    fn window_transition_settings_expand_focus_mode_from_main_preset() {
        let settings = window_transition_settings();

        assert_eq!(settings.main.width, 360);
        assert_eq!(settings.main.height, 420);
        assert_eq!(settings.focus.width, 540);
        assert_eq!(settings.focus.height, 840);
        assert_eq!(settings.duration_millis, 150);
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
                    "publish-bearer",
                    "git-http-bearer",
                    r#"{"token":"top-secret-token"}"#,
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
        connection
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    credentials_id
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "filesystem-release",
                    "filesystem",
                    publish_credentials_id,
                ],
            )
            .expect("publish target should insert");
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

        assert!(!inspection.generated_at.is_empty());
        assert_eq!(inspection.repositories.len(), 1);

        let repository = &inspection.repositories[0];
        assert_eq!(repository.repository_name, "repo-inspection");
        assert_eq!(repository.repo_url, "https://example.com/repo-inspection.git");
        assert_eq!(repository.polling_interval_seconds, 120);
        assert_eq!(repository.enabled_build_target_count, 1);
        assert_eq!(repository.build_targets.len(), 1);
        assert_eq!(repository.build_targets[0].target_name, "windows-player");
        assert_eq!(repository.publish_targets.len(), 1);
        assert_eq!(repository.publish_targets[0].name, "filesystem-release");
        assert_eq!(repository.pending_release_count, 1);
        assert_eq!(repository.release_queue.len(), 1);
        assert_eq!(repository.release_queue[0].git_tag, "v10.0.0");

        let repository_credentials = repository
            .credentials
            .as_ref()
            .expect("repository credentials should resolve");
        assert_eq!(repository_credentials.name, "origin-basic");
        assert_eq!(repository_credentials.kind, "git-http-basic");
        assert_eq!(repository_credentials.config_status, "ready");

        let publish_credentials = repository.publish_targets[0]
            .credentials
            .as_ref()
            .expect("publish target credentials should resolve");
        assert_eq!(publish_credentials.name, "publish-bearer");
        assert_eq!(publish_credentials.kind, "git-http-bearer");

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
        assert_eq!(feed.items[0].current_step_label, "Queued for build");
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
        assert_eq!(payload.report_path.as_deref(), Some(report_path.as_path()));
        assert!(payload.exists);
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
    fn persist_secret_bindings_updates_repository_and_publish_target_settings() {
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

        persist_repository_secret_binding(
            &config,
            UpdateRepositorySecretBindingInput {
                repository_id,
                credentials_id: Some(credentials_id),
            },
        )
        .expect("repository binding should persist");
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
        assert_eq!(settings.repository_bindings[0].credentials_id, Some(credentials_id));
        assert_eq!(settings.publish_target_bindings.len(), 1);
        assert_eq!(
            settings.publish_target_bindings[0].credentials_id,
            Some(credentials_id)
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
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
                repository_url: String::from("https://example.com/workers.git"),
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
    fn persist_repository_project_binds_default_github_auth_credential() {
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
                repository_url: String::from("https://github.com/indiegabo/workers.git"),
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
            },
        )
        .expect("repository project should persist");

        assert_eq!(created.credentials_id, Some(github_credentials_id));

        let inspection = load_repository_inspection(&config)
            .expect("repository inspection should reflect created project");

        assert_eq!(inspection.repositories.len(), 1);
        let repository = &inspection.repositories[0];
        let credentials = repository
            .credentials
            .as_ref()
            .expect("GitHub auth credential should be bound by default");
        assert_eq!(credentials.credential_id, github_credentials_id);
        assert_eq!(credentials.kind, "git-http-github-host-login");

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
                repository_url: String::from("https://github.com/indiegabo/workers.git"),
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
            },
        )
        .expect_err("PAT-based repository auth should be rejected");

        assert!(
            error
                .to_string()
                .contains("personal access token is no longer supported")
        );

        std::fs::remove_dir_all(root).expect("temp directory should be removable");
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
                repository_url: String::from("https://example.com/workers.git"),
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
                repository_url: String::from("https://example.com/workers.git"),
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
            },
        )
        .expect("repository project should persist");

        persist_repository_project_update(
            &config,
            UpdateRepositoryProjectCommandInput {
                repository_id: created.repository_id,
                name: String::from("Workers Updated"),
                engine_kind: String::from("unity"),
                repository_url: String::from("https://example.com/workers-updated.git"),
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
                repository_url: String::from("https://example.com/workers.git"),
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
            },
        )
        .expect("repository project should persist");

        let error = persist_repository_project_update(
            &config,
            UpdateRepositoryProjectCommandInput {
                repository_id: created.repository_id,
                name: String::from("Workers Updated"),
                engine_kind: String::from("unreal"),
                repository_url: String::from("https://example.com/workers-updated.git"),
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
}