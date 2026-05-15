//! Declares the durable store records, queue payloads, and command inputs used
//! across runtime-store coordination and reporting flows.

use super::*;

/// Defines the durable paths owned by the local runtime store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    pub database_path: PathBuf,
    pub health_report_path: PathBuf,
    pub supervision_contract_path: PathBuf,
    pub supervisor_state_path: PathBuf,
    pub runtime_events_path: PathBuf,
    pub runtime_events_cursor_path: PathBuf,
    pub runtime_control_requests_dir: PathBuf,
    pub runtime_log_path: PathBuf,
}

impl StorageLayout {
    /// Builds the store layout from the resolved runtime directories.
    pub fn from_directories(directories: &RuntimeDirectories) -> Self {
        Self {
            database_path: directories.state_dir.join("runtime.db"),
            health_report_path: directories.state_dir.join("health.json"),
            supervision_contract_path: directories.state_dir.join("supervision.json"),
            supervisor_state_path: directories.state_dir.join("supervisor-state.json"),
            runtime_events_path: directories.state_dir.join("runtime-events.jsonl"),
            runtime_events_cursor_path: directories.state_dir.join("runtime-events.cursor.json"),
            runtime_control_requests_dir: directories.state_dir.join("runtime-control"),
            runtime_log_path: directories.logs_dir.join("runtime.jsonl"),
        }
    }
}

/// Records one durable shell-to-runtime control request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeControlRequest {
    /// Forces one repository poll outside the normal in-memory schedule.
    ForceRepositoryPoll { repository_id: i64 },
}

/// Reports the SQLite bootstrap state produced by one runtime startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseBootstrapReport {
    pub database_path: PathBuf,
    pub busy_timeout_millis: u64,
    pub foreign_keys_enabled: bool,
    pub journal_mode: String,
    pub applied_migrations: Vec<String>,
}

/// Reports the reconciliation performed after the runtime restarts over local state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuntimeRecoveryReport {
    pub released_queue_message_leases: u64,
    pub cleared_coordination_leases: u64,
    pub requeued_build_runs: u64,
    pub requeued_publish_runs: u64,
    pub terminated_orphan_build_processes: u64,
    pub orphan_build_process_errors: u64,
    pub interrupted_builds: Vec<InterruptedBuildRecoveryRecord>,
}

/// Describes one interrupted build attempt captured during runtime recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterruptedBuildRecoveryRecord {
    pub build_run_id: i64,
    pub workspace_path: String,
    pub log_path: Option<String>,
    pub interruption_kind: String,
    pub interruption_message: String,
}

impl RuntimeRecoveryReport {
    /// Returns whether startup found no stale local work that required reconciliation.
    pub fn is_empty(&self) -> bool {
        self.released_queue_message_leases == 0
            && self.cleared_coordination_leases == 0
            && self.requeued_build_runs == 0
            && self.requeued_publish_runs == 0
            && self.terminated_orphan_build_processes == 0
            && self.orphan_build_process_errors == 0
            && self.interrupted_builds.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservedProcess {
    pub(crate) pid: u32,
    pub(crate) parent_pid: Option<u32>,
    pub(crate) name: String,
    pub(crate) command_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct OrphanBuildProcessTerminationReport {
    pub(crate) terminated_processes: u64,
    pub(crate) errors: u64,
}

/// Reports the current local automation backlog, queue, and coordination state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationSnapshot {
    pub generated_at: String,
    pub queue_messages: Vec<AutomationQueueSnapshot>,
    pub coordination_leases: Vec<AutomationCoordinationLeaseSnapshot>,
    pub repositories: Vec<RepositoryAutomationStatus>,
}

/// Summarizes the available and leased messages for one local worker queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationQueueSnapshot {
    pub queue_name: String,
    pub ready_count: i64,
    pub leased_count: i64,
}

/// Summarizes one active local coordination lease used by automation flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationCoordinationLeaseSnapshot {
    pub name: String,
    pub lease_expires_at_unix_millis: i64,
}

/// Reports the queued release and execution backlog currently attached to one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryAutomationStatus {
    pub repository_id: i64,
    pub repository_name: String,
    pub enabled: bool,
    pub polling_interval_seconds: i64,
    pub last_seen_tag: Option<String>,
    pub enabled_build_target_count: i64,
    pub pending_release_count: i64,
    pub queued_build_runs: i64,
    pub running_build_runs: i64,
    pub queued_publish_runs: i64,
    pub running_publish_runs: i64,
    pub release_queue: Vec<ReleaseAutomationStatus>,
}

/// Reports one queued release and the derived build and publish work it still blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseAutomationStatus {
    pub release_run_id: i64,
    pub git_tag: String,
    pub unity_version: Option<String>,
    pub status: String,
    pub planned: bool,
    pub build_process_active: bool,
    pub publish_process_active: bool,
    pub queued_build_runs: i64,
    pub running_build_runs: i64,
    pub terminal_build_runs: i64,
    pub total_build_runs: i64,
    pub queued_publish_runs: i64,
    pub running_publish_runs: i64,
    pub terminal_publish_runs: i64,
    pub total_publish_runs: i64,
}

/// Stores one queue message claimed under a renewable local SQLite lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedQueueMessage {
    pub id: i64,
    pub queue_name: String,
    pub payload: Vec<u8>,
    pub leased_by: String,
    pub lease_token: String,
    pub lease_expires_at_unix_millis: i64,
    pub dequeue_count: u32,
}

/// Stores one acquired exclusive lease for local coordination keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationLease {
    pub name: String,
    pub token: String,
    pub lease_expires_at_unix_millis: i64,
}

/// Describes the coordination outcome of one queue dispatch attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueDispatchOutcome {
    Enqueued,
    InProgress,
    AlreadyClaimed,
}

/// Encodes one build run into the queue payload consumed by local build workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildDispatchJob {
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub build_target_id: i64,
    pub unity_version: String,
    pub image_ref: String,
}

/// Encodes one release run into the queue payload consumed by local planners.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseDispatchJob {
    pub release_run_id: i64,
    pub repository_id: i64,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub trigger_source: String,
    pub trigger_rule_id: Option<i64>,
}

/// Encodes one publish run into the queue payload consumed by local publish workers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishDispatchJob {
    pub publish_run_id: i64,
    pub release_run_id: i64,
    pub build_run_id: i64,
    pub publish_target_id: i64,
    pub artifact_id: Option<i64>,
}

/// Stores one persisted build run returned by local release planning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRunRecord {
    pub id: i64,
    pub release_run_id: i64,
    pub build_target_id: i64,
    pub unity_version: Option<String>,
    pub image_ref: Option<String>,
    pub status: String,
    pub workspace_path: Option<String>,
    pub log_path: Option<String>,
    pub artifact_root_path: Option<String>,
    pub current_stage_key: Option<String>,
    pub current_stage_label: Option<String>,
    pub current_stage_status: Option<String>,
    pub heartbeat_at: Option<String>,
    pub last_progress_message: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Stores one durable stage record attached to a build run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRunStageRecord {
    pub id: i64,
    pub build_run_id: i64,
    pub position: i64,
    pub step_key: String,
    pub step_label: String,
    pub status: String,
    pub log_path: String,
    pub last_message: Option<String>,
    pub heartbeat_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Stores one durable artifact metadata row registered for a build run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: i64,
    pub build_run_id: i64,
    pub name: String,
    pub kind: String,
    pub path: String,
    pub size_bytes: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub created_at: String,
}

/// Stores one persisted release run handled by the local runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRunRecord {
    pub id: i64,
    pub repository_id: i64,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub trigger_source: String,
    pub trigger_rule_id: Option<i64>,
    pub source_metadata_json: String,
    pub unity_version: Option<String>,
    pub status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Stores one repository row exposed to the runtime polling loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollingRepositoryRecord {
    pub id: i64,
    pub name: String,
    pub repo_url: String,
    pub credentials_id: Option<i64>,
    pub enabled: bool,
    pub polling_interval_seconds: i64,
    pub last_seen_tag: Option<String>,
    pub default_branch: Option<String>,
    pub artifacts_root_override: Option<String>,
    pub workspace_root_override: Option<String>,
    pub enabled_build_target_count: i64,
    pub has_release_history: bool,
}

/// Stores one repository registration row needed to materialize a managed checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCheckoutRecord {
    pub id: i64,
    pub name: String,
    pub source_mode: String,
    pub workspace_strategy: String,
    pub repo_url: Option<String>,
    pub credentials_id: Option<i64>,
    pub default_branch: Option<String>,
    pub workspace_root_override: Option<String>,
    pub enabled: bool,
}

/// Reports one imported repository registration copied from another runtime database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedRepositoryRegistrationReport {
    pub source_database_path: String,
    pub repository_id: i64,
    pub repository_name: String,
    pub credential_name: Option<String>,
    pub trigger_rule_count: i64,
    pub build_target_count: i64,
    pub publish_target_count: i64,
    pub binding_count: i64,
}

/// Stores one durable publish run created from one build result and target binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRunRecord {
    pub id: i64,
    pub release_run_id: i64,
    pub build_run_id: i64,
    pub publish_target_id: i64,
    pub artifact_id: Option<i64>,
    pub status: String,
    pub destination_ref: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Joins the durable metadata required to execute one publish run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishExecutionPlan {
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

/// Stores one durable credentials row used by Git and publish authentication flows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRecord {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Joins the durable repository, release, and target metadata required to execute one build run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildExecutionPlan {
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub repository_credentials_id: Option<i64>,
    pub workspace_root_override: Option<String>,
    pub artifacts_root_override: Option<String>,
    pub build_target_id: i64,
    pub repository_url: String,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub target_name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_method: Option<String>,
    pub output_kind: Option<String>,
    pub output_path_template: Option<String>,
    pub config_json: String,
    pub unity_version: String,
    pub image_ref: String,
    pub timeout_seconds: i64,
    pub status: String,
}

/// Aggregates one release run into the operator-facing desktop home process feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFeedRecord {
    pub release_run_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub repository_url: String,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub unity_version: Option<String>,
    pub display_status: String,
    pub current_step_label: String,
    pub current_step_status: String,
    pub current_step_detail: Option<String>,
    pub queued_build_runs: i64,
    pub running_build_runs: i64,
    pub succeeded_build_runs: i64,
    pub failed_build_runs: i64,
    pub canceled_build_runs: i64,
    pub queued_publish_runs: i64,
    pub running_publish_runs: i64,
    pub succeeded_publish_runs: i64,
    pub failed_publish_runs: i64,
    pub canceled_publish_runs: i64,
    pub total_build_runs: i64,
    pub total_publish_runs: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Defines one paginated release-level process feed page for the desktop home view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessFeedPage {
    pub generated_at: String,
    pub page: u32,
    pub page_size: u32,
    pub total_items: i64,
    pub total_pages: u32,
    pub has_previous_page: bool,
    pub has_next_page: bool,
    pub items: Vec<ProcessFeedRecord>,
}

/// Joins the persisted build, release, repository, artifact, and publish metadata
/// needed by operator-facing build history surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildHistoryRecord {
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub repository_url: String,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub build_target_id: i64,
    pub build_target_name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_method: Option<String>,
    pub unity_version: Option<String>,
    pub image_ref: Option<String>,
    pub status: String,
    pub workspace_path: Option<String>,
    pub log_path: Option<String>,
    pub artifact_root_path: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub artifact_count: i64,
    pub publish_run_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Joins one persisted artifact with the release, build, and publish metadata
/// needed by operator-facing artifact inspection surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactInspectionRecord {
    pub artifact_id: i64,
    pub build_run_id: i64,
    pub release_run_id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub repository_url: String,
    pub git_tag: String,
    pub git_commit: Option<String>,
    pub build_target_id: i64,
    pub build_target_name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_status: String,
    pub artifact_name: String,
    pub artifact_kind: String,
    pub artifact_path: String,
    pub artifact_root_path: Option<String>,
    pub size_bytes: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub publish_run_count: i64,
    pub queued_publish_runs: i64,
    pub running_publish_runs: i64,
    pub succeeded_publish_runs: i64,
    pub failed_publish_runs: i64,
    pub canceled_publish_runs: i64,
    pub created_at: String,
}

/// Stores one persisted build target row exposed to shell settings and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildTargetRuntimeSettingsRecord {
    pub id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_method: Option<String>,
    pub enabled: bool,
    pub config_json: String,
}

/// Stores one persisted publish target row exposed to shell settings and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishTargetRuntimeSettingsRecord {
    pub id: i64,
    pub repository_id: i64,
    pub repository_name: String,
    pub name: String,
    pub kind: String,
    pub credentials_id: Option<i64>,
    pub enabled: bool,
}

/// Defines the persisted execution paths recorded when a build worker claims one queued run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartBuildRunInput {
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
}

/// Defines the persisted execution paths recorded when a running build run succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteBuildRunInput {
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
}

/// Defines the persisted execution paths and terminal error stored for a failed build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailBuildRunInput {
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub error_message: String,
}

/// Defines the persisted execution paths and terminal reason stored for a canceled build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelBuildRunInput {
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub error_message: String,
}

/// Starts or restarts one durable stage under a running build run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartBuildRunStageInput {
    pub position: i64,
    pub step_key: String,
    pub step_label: String,
    pub step_log_path: String,
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub message: String,
}

/// Refreshes the heartbeat and message of one running build stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatBuildRunStageInput {
    pub step_key: String,
    pub step_label: String,
    pub step_log_path: String,
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub message: String,
}

/// Marks one build stage as completed successfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteBuildRunStageInput {
    pub step_key: String,
    pub step_label: String,
    pub step_log_path: String,
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub message: String,
}

/// Marks one build stage as failed and stores the terminal message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailBuildRunStageInput {
    pub step_key: String,
    pub step_label: String,
    pub step_log_path: String,
    pub workspace_path: String,
    pub log_path: String,
    pub artifact_root_path: String,
    pub error_message: String,
}

/// Defines one artifact metadata row that must replace the current build artifact set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateArtifactRecordInput {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub size_bytes: Option<i64>,
    pub checksum_sha256: Option<String>,
}

/// Defines the create-or-update payload for one persisted credentials row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertCredentialRecordInput {
    pub credential_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub config_json: String,
}

/// Defines one credentials row that must be created alongside a repository project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryProjectCredentialInput {
    pub name: String,
    pub kind: String,
    pub config_json: String,
}

/// Defines one build target that must be attached to a new repository project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryProjectBuildTargetInput {
    pub name: String,
    pub platform: String,
    pub runner_type: String,
    pub build_method: String,
    pub output_kind: Option<String>,
    pub output_path_template: Option<String>,
    pub unity_version_override: Option<String>,
    pub timeout_seconds: i64,
    pub enabled: bool,
    pub config_json: String,
}

/// Defines the durable payload required to register one managed repository project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRepositoryProjectInput {
    pub name: String,
    pub repo_url: String,
    pub credentials: Option<CreateRepositoryProjectCredentialInput>,
    pub default_branch: Option<String>,
    pub artifacts_root_override: Option<String>,
    pub workspace_root_override: Option<String>,
    pub polling_interval_seconds: i64,
    pub enabled: bool,
    pub build_targets: Vec<CreateRepositoryProjectBuildTargetInput>,
}

/// Defines the durable payload required to update one managed repository
/// project without replacing its worker or credentials bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateRepositoryProjectInput {
    pub repository_id: i64,
    pub name: String,
    pub repo_url: String,
    pub default_branch: Option<String>,
    pub artifacts_root_override: Option<String>,
    pub workspace_root_override: Option<String>,
    pub polling_interval_seconds: i64,
    pub enabled: bool,
}

/// Reports one repository project created through the operator wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedRepositoryProjectRecord {
    pub repository_id: i64,
    pub repository_name: String,
    pub credentials_id: Option<i64>,
    pub build_target_ids: Vec<i64>,
}

/// Defines the queued-to-running transition input for one publish run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StartPublishRunInput {}

/// Defines the completion fields persisted when one publish run succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletePublishRunInput {
    pub destination_ref: String,
}

/// Defines the terminal failure fields persisted when one publish run fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailPublishRunInput {
    pub destination_ref: String,
    pub error_message: String,
}

/// Defines the operator-provided fields for one manual release dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualReleaseDispatchInput {
    pub repository_id: i64,
    pub git_tag: String,
    pub git_commit: String,
    pub requested_via: String,
}

/// Defines one release candidate discovered through repository polling automation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryPollDispatchInput {
    pub repository_id: i64,
    pub git_tag: String,
    pub git_commit: String,
    pub observed_via: String,
}
