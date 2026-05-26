//! Persists runtime state, SQLite-backed coordination primitives, execution
//! metadata, and operator-facing inspection queries for the local host.

#![forbid(unsafe_code)]

pub mod lifecycle;
mod models;

pub use models::*;

use crate::lifecycle::{BuildStatus, PublishStatus, ReleaseStatus};
use keyring::Entry;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use runtime_config::{RuntimeConcurrencySettings, RuntimeDirectories};
use runtime_contracts::{BuildKind, EngineKind};
use runtime_git::{
    git_auth_options_from_credentials, GitAuthOptions,
    KIND_GIT_HTTP_BASIC, KIND_GIT_HTTP_BEARER,
};
use serde::{Deserialize, Serialize};
use sysinfo::System;

/// Sets the bounded SQLite lock wait used by the local runtime.
pub const SQLITE_BUSY_TIMEOUT_MILLIS: u64 = 5_000;

const COORDINATION_POLL_INTERVAL_MILLIS: u64 = 25;
const RELEASE_RUN_QUEUE_NAME: &str = "release-runs";
const BUILD_RUN_QUEUE_NAME: &str = "build-runs";
const PUBLISH_RUN_QUEUE_NAME: &str = "publish-runs";
const DISPATCH_LOCK_TTL: Duration = Duration::from_secs(30);
const RELEASE_PLANNING_LOCK_TTL: Duration = Duration::from_secs(30 * 60);
const DISPATCH_IDEMPOTENCY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_HOST_NATIVE_RUNNER_TYPE: &str = "host-native";
const DEFAULT_REPOSITORY_POLL_TRIGGER_RULE_NAME: &str = "poll-release-tags";
pub const HOST_KEYRING_SERVICE: &str = "handy-games-publisher";
pub const KEYRING_SECRET_REF_PREFIX: &str = "keyring://";
pub const KIND_ITCH_API_KEY: &str = "itch-api-key";
const REPOSITORY_AUTH_BINDING_STATUS_REAUTH_REQUIRED: &str = "reauth_required";
const SUPPORTED_REPOSITORY_ENGINE_UNITY: &str = "unity";
const SUPPORTED_REPOSITORY_BUILD_KIND_PLAYER: &str = "player";
const SOURCE_MODE_MANAGED_REPOSITORY: &str = "managed_repository";
const SOURCE_MODE_LOCAL_WORKSPACE: &str = "local_workspace";
const WORKSPACE_STRATEGY_DIRECT: &str = "direct";
const WORKSPACE_STRATEGY_MANAGED_CHECKOUT: &str = "managed_checkout";
const TRIGGER_SOURCE_MANUAL: &str = "manual";
const TRIGGER_SOURCE_POLL: &str = "poll";
const TRIGGER_SOURCE_REPOSITORY_POLL: &str = "repository-poll";
const RELEASE_SOURCE_KIND_MANAGED_TAG: &str = "managed_tag";
const RELEASE_SOURCE_KIND_MANAGED_REF: &str = "managed_ref";
const RELEASE_SOURCE_KIND_LOCAL_WORKSPACE: &str = "local_workspace";
const RELEASE_VERSION_SOURCE_MANUAL: &str = "manual";
const RELEASE_VERSION_SOURCE_PROJECT_SETTINGS: &str = "project_settings";
const PROJECT_VERSION_FILE_PATH: &str = "ProjectSettings/ProjectVersion.txt";
const PROJECT_SETTINGS_ASSET_PATH: &str = "ProjectSettings/ProjectSettings.asset";

pub const RECOVERY_INTERRUPTION_KIND_REQUESTED: &str = "requested_shutdown";
pub const RECOVERY_INTERRUPTION_KIND_SYSTEM: &str = "system_interruption";

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryProjectRemovalRecord {
    id: i64,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetQueueMessage {
    id: i64,
    leased_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryProjectRemovalRuntimeState {
    release_run_ids: HashSet<i64>,
    build_run_ids: HashSet<i64>,
    publish_run_ids: HashSet<i64>,
    queue_messages: Vec<TargetQueueMessage>,
    coordination_lease_names: Vec<String>,
    idempotency_keys: Vec<String>,
    directory_paths: Vec<String>,
    file_paths: Vec<String>,
}

struct Migration {
    name: &'static str,
    sql: &'static str,
    transactional: bool,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_runtime_metadata.sql",
        sql: include_str!("../migrations/0001_runtime_metadata.sql"),
        transactional: true,
    },
    Migration {
        name: "0002_pipeline_definitions.sql",
        sql: include_str!("../migrations/0002_pipeline_definitions.sql"),
        transactional: true,
    },
    Migration {
        name: "0003_execution_runs.sql",
        sql: include_str!("../migrations/0003_execution_runs.sql"),
        transactional: true,
    },
    Migration {
        name: "0004_local_coordination.sql",
        sql: include_str!("../migrations/0004_local_coordination.sql"),
        transactional: true,

    },
    Migration {
        name: "0005_host_native_runner_defaults.sql",
        sql: include_str!("../migrations/0005_host_native_runner_defaults.sql"),
        transactional: true,
    },
    Migration {
        name: "0006_repository_source_configuration.sql",
        sql: include_str!("../migrations/0006_repository_source_configuration.sql"),
        transactional: false,
    },
    Migration {
        name: "0007_repository_path_model_cleanup.sql",
        sql: include_str!("../migrations/0007_repository_path_model_cleanup.sql"),
        transactional: false,
    },
    Migration {
        name: "0008_build_run_stage_tracking.sql",
        sql: include_str!("../migrations/0008_build_run_stage_tracking.sql"),
        transactional: true,
    },
    Migration {
        name: "0009_build_target_runner_model_cleanup.sql",
        sql: include_str!("../migrations/0009_build_target_runner_model_cleanup.sql"),
        transactional: false,
    },
    Migration {
        name: "0009_execution_retention.sql",
        sql: include_str!("../migrations/0009_execution_retention.sql"),
        transactional: true,
    },
    Migration {
        name: "0010_engine_contract_model.sql",
        sql: include_str!("../migrations/0010_engine_contract_model.sql"),
        transactional: true,
    },
    Migration {
        name: "0011_runtime_engine_version.sql",
        sql: include_str!("../migrations/0011_runtime_engine_version.sql"),
        transactional: true,
    },
    Migration {
        name: "0012_repository_auth_state.sql",
        sql: include_str!("../migrations/0012_repository_auth_state.sql"),
        transactional: true,
    },
    Migration {
        name: "0013_publish_destination_execution_contract.sql",
        sql: include_str!("../migrations/0013_publish_destination_execution_contract.sql"),
        transactional: true,
    },
    Migration {
        name: "0014_release_source_identity.sql",
        sql: include_str!("../migrations/0014_release_source_identity.sql"),
        transactional: true,
    },
    Migration {
        name: "0015_local_workspace_polling_invariant.sql",
        sql: include_str!("../migrations/0015_local_workspace_polling_invariant.sql"),
        transactional: false,
    },
];

const MIGRATION_NO_OP_SQL: &str = "SELECT 1;\n";

const RENAME_RELEASE_RUN_ENGINE_VERSION_SQL: &str = r#"
ALTER TABLE release_runs RENAME COLUMN unity_version TO engine_version;
"#;

const RENAME_BUILD_RUN_ENGINE_VERSION_SQL: &str = r#"
ALTER TABLE build_runs RENAME COLUMN unity_version TO engine_version;
"#;

#[allow(unused_imports)]
use models::{ObservedProcess, OrphanBuildProcessTerminationReport};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct BuildQueueJob {
    build_run_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PublishQueueJob {
    publish_run_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseRunReconciliation {
    status: String,
    started_at: Option<String>,
    finished_at: Option<String>,
    error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AvailableQueueMessage {
    id: i64,
    payload: Vec<u8>,
    dequeue_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRunClaimState {
    release_run_id: i64,
    repository_id: i64,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishRunClaimState {
    artifact_id: Option<i64>,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRunDispatchState {
    job: BuildDispatchJob,
    status: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishRunDispatchState {
    job: PublishDispatchJob,
    status: String,
    created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseBuildPlanningState {
    repository_id: i64,
    repository_source_mode: String,
    repository_url: String,
    repository_local_path: Option<String>,
    engine_kind: EngineKind,
    credentials_id: Option<i64>,
    git_tag: String,
    source_metadata_json: String,
    engine_version: Option<String>,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnDemandRepositoryState {
    repository_id: i64,
    source_mode: String,
    repo_url: Option<String>,
    local_path: Option<String>,
    credentials_id: Option<i64>,
    default_branch: Option<String>,
    engine_kind: EngineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedReleaseSourceMetadata {
    requested_via: Option<String>,
    observed_via: Option<String>,
    source_kind: String,
    source_ref: Option<String>,
    local_path: Option<String>,
    version_source: Option<String>,
    unity_executable_path_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildTargetPlanningState {
    id: i64,
    build_kind: BuildKind,
    contract_json: String,
    runner_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepositoryProjectBuildContractInput {
    #[serde(default)]
    unity: Option<UnityRepositoryProjectBuildContractInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnityRepositoryProjectBuildContractInput {
    #[serde(rename = "targetPlatform", default)]
    target_platform: String,
    #[serde(rename = "buildMethod", default)]
    build_method: String,
    #[serde(rename = "editorVersion", default)]
    editor_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildTargetReadModelProjection {
    unity_target_platform: String,
    unity_build_method: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AutomationRepositoryRow {
    id: i64,
    name: String,
    enabled: bool,
    polling_interval_seconds: i64,
    last_seen_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseJobDisposition {
    Acknowledge,
    RetryLater,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedManualReleaseDispatchInput {
    repository_id: i64,
    git_tag: String,
    git_commit: String,
    requested_via: String,
    source_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedRepositoryPollDispatchInput {
    repository_id: i64,
    git_tag: String,
    git_commit: String,
    observed_via: String,
    source_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedOnDemandReleaseDispatchInput {
    repository_id: i64,
    git_tag: String,
    git_commit: String,
    requested_via: String,
    source_kind: String,
    source_ref: Option<String>,
    local_path: Option<String>,
    version_source: String,
    unity_executable_path_override: Option<String>,
    source_identity: String,
}

/// Owns the local queue, lease, and idempotency primitives backed by SQLite.
///
/// Each operation opens a short-lived SQLite connection so the runtime can use
/// the coordinator across command boundaries without sharing one mutable handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCoordinator {
    database_path: PathBuf,
}

impl LocalCoordinator {
    /// Builds a local coordination helper from the runtime storage layout.
    pub fn new(storage: &StorageLayout) -> Self {
        Self {
            database_path: storage.database_path.clone(),
        }
    }

    /// Appends one durable payload to the named local worker queue.
    pub fn enqueue(&self, queue_name: &str, payload: &[u8]) -> io::Result<()> {
        let queue_name = require_non_empty(queue_name, "queue name")?;
        if payload.is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "queue payload must not be empty",
            ));
        }

        let connection = open_connection(&self.database_path)?;
        connection
            .execute(
                "
                INSERT INTO worker_queue_messages (queue_name, payload)
                VALUES (?, ?)
                ",
                params![queue_name, payload],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    /// Claims the next available queue message under a renewable lease.
    pub fn claim_next(
        &self,
        queue_name: &str,
        worker_name: &str,
        wait: Duration,
        lease_ttl: Duration,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let queue_name = require_non_empty(queue_name, "queue name")?;
        let worker_name = require_non_empty(worker_name, "worker name")?;
        let lease_ttl_millis = duration_to_millis(lease_ttl, "queue lease ttl")?;
        let started_at = Instant::now();

        loop {
            if let Some(message) = self.claim_next_once(
                &queue_name,
                &worker_name,
                lease_ttl_millis,
            )? {
                return Ok(Some(message));
            }

            if started_at.elapsed() >= wait {
                return Ok(None);
            }

            thread::sleep(next_poll_interval(wait, started_at.elapsed()));
        }
    }

    /// Claims the next eligible build job while respecting host-local limits.
    pub fn claim_next_build_job(
        &self,
        worker_name: &str,
        wait: Duration,
        lease_ttl: Duration,
        concurrency: &RuntimeConcurrencySettings,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let worker_name = require_non_empty(worker_name, "worker name")?;
        let lease_ttl_millis = duration_to_millis(lease_ttl, "queue lease ttl")?;
        let started_at = Instant::now();

        loop {
            if let Some(message) = self.claim_next_build_job_once(
                &worker_name,
                lease_ttl_millis,
                concurrency,
            )? {
                return Ok(Some(message));
            }

            if started_at.elapsed() >= wait {
                return Ok(None);
            }

            thread::sleep(next_poll_interval(wait, started_at.elapsed()));
        }
    }

    /// Claims the next eligible publish job while respecting host-local limits.
    pub fn claim_next_publish_job(
        &self,
        worker_name: &str,
        wait: Duration,
        lease_ttl: Duration,
        concurrency: &RuntimeConcurrencySettings,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let worker_name = require_non_empty(worker_name, "worker name")?;
        let lease_ttl_millis = duration_to_millis(lease_ttl, "queue lease ttl")?;
        let started_at = Instant::now();

        loop {
            if let Some(message) = self.claim_next_publish_job_once(
                &worker_name,
                lease_ttl_millis,
                concurrency,
            )? {
                return Ok(Some(message));
            }

            if started_at.elapsed() >= wait {
                return Ok(None);
            }

            thread::sleep(next_poll_interval(wait, started_at.elapsed()));
        }
    }

    /// Extends the lease of one already-claimed queue message.
    pub fn renew_message_lease(
        &self,
        message_id: i64,
        lease_token: &str,
        lease_ttl: Duration,
    ) -> io::Result<bool> {
        let lease_token = require_non_empty(lease_token, "queue lease token")?;
        let lease_ttl_millis = duration_to_millis(lease_ttl, "queue lease ttl")?;
        let now = unix_timestamp_millis()?;
        let next_expiry = now + lease_ttl_millis;
        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE worker_queue_messages
                SET lease_expires_at_unix_millis = ?
                WHERE id = ?
                  AND lease_token = ?
                  AND lease_expires_at_unix_millis > ?
                ",
                params![next_expiry, message_id, lease_token, now],
            )
            .map_err(sqlite_error)?;

        Ok(updated == 1)
    }

    /// Acknowledges one claimed queue message and removes it from the queue.
    pub fn acknowledge_message(&self, message_id: i64, lease_token: &str) -> io::Result<bool> {
        let lease_token = require_non_empty(lease_token, "queue lease token")?;
        let connection = open_connection(&self.database_path)?;
        let deleted = connection
            .execute(
                "DELETE FROM worker_queue_messages WHERE id = ? AND lease_token = ?",
                params![message_id, lease_token],
            )
            .map_err(sqlite_error)?;

        Ok(deleted == 1)
    }

    /// Releases one claimed queue message back into the available pool.
    pub fn release_message(&self, message_id: i64, lease_token: &str) -> io::Result<bool> {
        let lease_token = require_non_empty(lease_token, "queue lease token")?;
        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE worker_queue_messages
                SET leased_by = NULL,
                    lease_token = NULL,
                    lease_expires_at_unix_millis = NULL
                WHERE id = ? AND lease_token = ?
                ",
                params![message_id, lease_token],
            )
            .map_err(sqlite_error)?;

        Ok(updated == 1)
    }

    /// Attempts to acquire one exclusive local coordination lease.
    pub fn acquire_lock(&self, name: &str, ttl: Duration) -> io::Result<Option<CoordinationLease>> {
        let name = require_non_empty(name, "lock name")?;
        let ttl_millis = duration_to_millis(ttl, "lock ttl")?;
        let now = unix_timestamp_millis()?;
        let expiry = now + ttl_millis;
        let token = next_token("lock")?;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                DELETE FROM worker_coordination_leases
                WHERE name = ? AND lease_expires_at_unix_millis <= ?
                ",
                params![name, now],
            )
            .map_err(sqlite_error)?;
        let inserted = transaction
            .execute(
                "
                INSERT OR IGNORE INTO worker_coordination_leases (
                    name,
                    token,
                    lease_expires_at_unix_millis
                )
                VALUES (?, ?, ?)
                ",
                params![name, token, expiry],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;

        if inserted == 0 {
            return Ok(None);
        }

        Ok(Some(CoordinationLease {
            name,
            token,
            lease_expires_at_unix_millis: expiry,
        }))
    }

    /// Extends one owned coordination lease when it has not yet expired.
    pub fn renew_lock(&self, name: &str, token: &str, ttl: Duration) -> io::Result<bool> {
        let name = require_non_empty(name, "lock name")?;
        let token = require_non_empty(token, "lock token")?;
        let ttl_millis = duration_to_millis(ttl, "lock ttl")?;
        let now = unix_timestamp_millis()?;
        let expiry = now + ttl_millis;
        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE worker_coordination_leases
                SET lease_expires_at_unix_millis = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE name = ?
                  AND token = ?
                  AND lease_expires_at_unix_millis > ?
                ",
                params![expiry, name, token, now],
            )
            .map_err(sqlite_error)?;

        Ok(updated == 1)
    }

    /// Releases one owned coordination lease.
    pub fn release_lock(&self, name: &str, token: &str) -> io::Result<bool> {
        let name = require_non_empty(name, "lock name")?;
        let token = require_non_empty(token, "lock token")?;
        let connection = open_connection(&self.database_path)?;
        let deleted = connection
            .execute(
                "DELETE FROM worker_coordination_leases WHERE name = ? AND token = ?",
                params![name, token],
            )
            .map_err(sqlite_error)?;

        Ok(deleted == 1)
    }

    /// Claims one idempotency key for a bounded retry window.
    pub fn claim_idempotency(&self, key: &str, ttl: Duration) -> io::Result<bool> {
        let key = require_non_empty(key, "idempotency key")?;
        let ttl_millis = duration_to_millis(ttl, "idempotency ttl")?;
        let now = unix_timestamp_millis()?;
        let expiry = now + ttl_millis;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                DELETE FROM worker_idempotency_keys
                WHERE idempotency_key = ? AND claim_expires_at_unix_millis <= ?
                ",
                params![key, now],
            )
            .map_err(sqlite_error)?;
        let inserted = transaction
            .execute(
                "
                INSERT OR IGNORE INTO worker_idempotency_keys (
                    idempotency_key,
                    claim_expires_at_unix_millis
                )
                VALUES (?, ?)
                ",
                params![key, expiry],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(inserted == 1)
    }

    /// Forgets one idempotency key so a failed enqueue path can be retried.
    pub fn forget_idempotency(&self, key: &str) -> io::Result<()> {
        let key = require_non_empty(key, "idempotency key")?;
        let connection = open_connection(&self.database_path)?;
        connection
            .execute(
                "DELETE FROM worker_idempotency_keys WHERE idempotency_key = ?",
                params![key],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    /// Serializes one queued build run into the local build queue with dispatch coordination.
    pub fn dispatch_build_run(&self, build_run_id: i64) -> io::Result<QueueDispatchOutcome> {
        require_positive_identifier(build_run_id, "build run id")?;

        let lock_name = build_dispatch_lock_key(build_run_id);
        let Some(lock) = self.acquire_lock(&lock_name, DISPATCH_LOCK_TTL)? else {
            return Ok(QueueDispatchOutcome::InProgress);
        };

        let outcome = self.dispatch_build_run_locked(build_run_id);
        let _ = self.release_lock(&lock.name, &lock.token);

        outcome
    }

    /// Serializes one queued publish run into the local publish queue with dispatch coordination.
    pub fn dispatch_publish_run(&self, publish_run_id: i64) -> io::Result<QueueDispatchOutcome> {
        require_positive_identifier(publish_run_id, "publish run id")?;

        let lock_name = publish_dispatch_lock_key(publish_run_id);
        let Some(lock) = self.acquire_lock(&lock_name, DISPATCH_LOCK_TTL)? else {
            return Ok(QueueDispatchOutcome::InProgress);
        };

        let outcome = self.dispatch_publish_run_locked(publish_run_id);
        let _ = self.release_lock(&lock.name, &lock.token);

        outcome
    }

    /// Persists one manual release run and queues it for downstream planning.
    pub fn dispatch_manual_release(
        &self,
        input: ManualReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        let input = normalize_manual_release_dispatch_input(input)?;
        let record = self.create_manual_release_dispatch(&input)?;

        self.queue_release_run(record.id)
    }

    /// Reuses an existing manual release row for the same repository tag and requeues it.
    pub fn dispatch_manual_release_rebuild(
        &self,
        input: ManualReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        let input = normalize_manual_release_dispatch_input(input)?;
        let record = self.rebuild_manual_release_dispatch(&input)?;
        self.forget_idempotency(&release_dispatch_idempotency_key(record.id))?;

        self.queue_release_run(record.id)
    }

    /// Persists one on-demand release run and queues it for downstream planning.
    pub fn dispatch_on_demand_release(
        &self,
        input: OnDemandReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        let input = self.normalize_on_demand_release_dispatch_input(input)?;
        let record = self.create_on_demand_release_dispatch(&input)?;

        self.queue_release_run(record.id)
    }

    /// Requeues one stored release run while preserving its original source metadata.
    pub fn dispatch_release_rebuild_by_id(
        &self,
        release_run_id: i64,
        requested_via: &str,
    ) -> io::Result<ReleaseRunRecord> {
        require_positive_identifier(release_run_id, "release run id")?;

        let record = self.rebuild_release_dispatch_by_id(release_run_id, requested_via)?;
        self.forget_idempotency(&release_dispatch_idempotency_key(record.id))?;

        self.queue_release_run(record.id)
    }

    /// Persists one repository-poll release run and queues it for downstream planning.
    pub fn dispatch_repository_poll_release(
        &self,
        input: RepositoryPollDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        let input = normalize_repository_poll_dispatch_input(input)?;
        let record = self.create_repository_poll_dispatch(&input)?;

        self.queue_release_run(record.id)
    }

    /// Hands one stored release run to the local release queue and marks it queued.
    pub fn queue_release_run(&self, release_run_id: i64) -> io::Result<ReleaseRunRecord> {
        require_positive_identifier(release_run_id, "release run id")?;
        let record = self
            .load_release_run_record(release_run_id)?
            .ok_or_else(|| not_found_error(format!("release run {release_run_id} was not found")))?;
        if record.status == ReleaseStatus::Queued.as_str() {
            return Ok(record);
        }

        let lock_name = release_dispatch_lock_key(release_run_id);
        let Some(lock) = self.acquire_lock(&lock_name, DISPATCH_LOCK_TTL)? else {
            return Err(io::Error::new(
                ErrorKind::WouldBlock,
                format!("release run {release_run_id} dispatch is already in progress"),
            ));
        };

        let outcome = self.queue_release_run_locked(release_run_id);
        let _ = self.release_lock(&lock.name, &lock.token);

        outcome
    }

    /// Loads one persisted release run by identifier.
    pub fn get_release_run_record(&self, release_run_id: i64) -> io::Result<ReleaseRunRecord> {
        require_positive_identifier(release_run_id, "release run id")?;
        self.load_release_run_record(release_run_id)?.ok_or_else(|| {
            not_found_error(format!("release run {release_run_id} was not found"))
        })
    }

    /// Plans queued build runs for one queued release and dispatches queued work.
    pub fn plan_release_builds(&self, release_run_id: i64) -> io::Result<Vec<BuildRunRecord>> {
        require_positive_identifier(release_run_id, "release run id")?;

        let mut connection = open_connection(&self.database_path)?;
        let release = self
            .load_release_build_planning_state_from_connection(&connection, release_run_id)?
            .ok_or_else(|| not_found_error(format!("release run {release_run_id} was not found")))?;
        if release.status != ReleaseStatus::Queued.as_str() {
            return Err(invalid_input_error(format!(
                "release run {release_run_id} must be queued before build planning"
            )));
        }

        let initial_release_engine_version = self.resolve_release_engine_version(
            release_run_id,
            &release,
        )?;

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let release = self
            .load_release_build_planning_state(&transaction, release_run_id)?
            .ok_or_else(|| not_found_error(format!("release run {release_run_id} was not found")))?;
        if release.status != ReleaseStatus::Queued.as_str() {
            return Err(invalid_input_error(format!(
                "release run {release_run_id} must be queued before build planning"
            )));
        }

        let release_engine_version = release
            .engine_version
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(initial_release_engine_version.as_str());
        if release_engine_version.is_empty() {
            return Err(invalid_input_error(format!(
                "release run {release_run_id} is missing engine version for build planning"
            )));
        }

        let targets = self.list_enabled_build_targets_for_planning(
            &transaction,
            release.repository_id,
        )?;
        if targets.is_empty() {
            return Err(invalid_input_error(format!(
                "release run {release_run_id} has no enabled build targets"
            )));
        }

        for target in &targets {
            let engine_version = resolve_target_engine_version(
                target,
                release.engine_kind,
                release_engine_version,
            )?;
            let image_ref = resolve_build_image_ref(target, &engine_version)?;
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
                    ON CONFLICT(release_run_id, build_target_id) DO UPDATE SET
                        engine_version = excluded.engine_version,
                        image_ref = excluded.image_ref,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE build_runs.status = ?
                    ",
                    params![
                        release_run_id,
                        target.id,
                        engine_version,
                        image_ref,
                        BuildStatus::Queued.as_str(),
                        BuildStatus::Queued.as_str(),
                    ],
                )
                .map_err(sqlite_error)?;
        }

        let runs = self.list_build_runs_by_release(&transaction, release_run_id)?;
        transaction.commit().map_err(sqlite_error)?;

        self.dispatch_next_build_run_for_release(release_run_id)?;

        Ok(runs)
    }

    /// Loads the joined metadata required to execute one planned build run.
    pub fn get_build_execution_plan(&self, build_run_id: i64) -> io::Result<BuildExecutionPlan> {
        require_positive_identifier(build_run_id, "build run id")?;

        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT br.id,
                       br.release_run_id,
                       rr.repository_id,
                      r.engine_kind,
                       r.name,
                       r.credentials_id,
                      r.workspace_root_override,
                      r.artifacts_root_override,
                       br.build_target_id,
                      r.source_mode,
                       r.repo_url,
                      r.local_path,
                       rr.git_tag,
                       rr.git_commit,
                      rr.source_metadata_json,
                       bt.name,
                      COALESCE(bt.build_kind, ''),
                      COALESCE(bt.contract_json, ''),
                       bt.runner_type,
                       bt.output_kind,
                       bt.output_path_template,
                       bt.config_json,
                      br.engine_version,
                       br.image_ref,
                       bt.timeout_seconds,
                       br.status
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                JOIN repositories r ON r.id = rr.repository_id
                JOIN build_targets bt ON bt.id = br.build_target_id
                WHERE br.id = ?
                ",
                [build_run_id],
                scan_build_execution_plan,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    not_found_error(format!("build run {build_run_id} was not found"))
                }
                other => sqlite_error(other),
            })
    }

    /// Loads one stored credentials row by identifier.
    pub fn get_credential_record(
        &self,
        credentials_id: i64,
    ) -> io::Result<CredentialRecord> {
        require_positive_identifier(credentials_id, "credentials id")?;

        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
                       name,
                       kind,
                       config_json,
                       created_at,
                       updated_at
                FROM credentials
                WHERE id = ?
                ",
                [credentials_id],
                scan_credential_record,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    not_found_error(format!("credentials {credentials_id} were not found"))
                }
                other => sqlite_error(other),
            })
    }

    /// Creates or updates one stored credentials row and returns the persisted record.
    pub fn upsert_credential_record(
        &self,
        input: UpsertCredentialRecordInput,
    ) -> io::Result<CredentialRecord> {
        let normalized = normalize_upsert_credential_record_input(input)?;
        let connection = open_connection(&self.database_path)?;

        let credential_id = if let Some(credential_id) = normalized.credential_id {
            let updated = connection
                .execute(
                    "
                    UPDATE credentials
                    SET name = ?,
                        kind = ?,
                        config_json = ?,
                        updated_at = CURRENT_TIMESTAMP
                    WHERE id = ?
                    ",
                    params![
                        normalized.name,
                        normalized.kind,
                        normalized.config_json,
                        credential_id,
                    ],
                )
                .map_err(sqlite_error)?;
            if updated == 0 {
                return Err(not_found_error(format!(
                    "credentials {credential_id} were not found"
                )));
            }

            credential_id
        } else {
            connection
                .execute(
                    "
                    INSERT INTO credentials (name, kind, config_json)
                    VALUES (?, ?, ?)
                    ",
                    params![
                        normalized.name,
                        normalized.kind,
                        normalized.config_json,
                    ],
                )
                .map_err(sqlite_error)?;
            connection.last_insert_rowid()
        };

        self.get_credential_record(credential_id)
    }

    /// Creates one managed repository project and all of its build targets in one transaction.
    pub fn create_repository_project(
        &self,
        input: CreateRepositoryProjectInput,
    ) -> io::Result<CreatedRepositoryProjectRecord> {
        let normalized = normalize_create_repository_project_input(input)?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        reject_duplicate_repository_project_name(&transaction, &normalized.name)?;
        match normalized.source_mode.as_str() {
            SOURCE_MODE_MANAGED_REPOSITORY => reject_duplicate_repository_project_url(
                &transaction,
                normalized.repo_url.as_deref().ok_or_else(|| {
                    invalid_input_error("repository project repo_url must not be empty")
                })?,
            )?,
            SOURCE_MODE_LOCAL_WORKSPACE => reject_duplicate_repository_project_local_path(
                &transaction,
                normalized.local_path.as_deref().ok_or_else(|| {
                    invalid_input_error("repository project local_path must not be empty")
                })?,
            )?,
            _ => {
                return Err(invalid_input_error(format!(
                    "repository project source_mode {:?} is not supported",
                    normalized.source_mode
                )));
            }
        }

        let credentials_id = if let Some(credentials) = normalized.credentials {
            transaction
                .execute(
                    "
                    INSERT INTO credentials (name, kind, config_json)
                    VALUES (?, ?, ?)
                    ",
                    params![
                        credentials.name,
                        credentials.kind,
                        credentials.config_json,
                    ],
                )
                .map_err(sqlite_error)?;
            Some(transaction.last_insert_rowid())
        } else {
            None
        };

        transaction
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    source_mode,
                    workspace_strategy,
                    repo_url,
                    local_path,
                    credentials_id,
                    default_branch,
                    artifacts_root_override,
                    workspace_root_override,
                    polling_interval_seconds,
                    last_seen_tag,
                    engine_kind,
                    enabled
                )
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
                ",
                params![
                    normalized.name,
                    normalized.source_mode,
                    if normalized.source_mode == SOURCE_MODE_LOCAL_WORKSPACE {
                        WORKSPACE_STRATEGY_DIRECT
                    } else {
                        WORKSPACE_STRATEGY_MANAGED_CHECKOUT
                    },
                    normalized.repo_url,
                    normalized.local_path,
                    credentials_id,
                    normalized.default_branch,
                    normalized.artifacts_root_override,
                    normalized.workspace_root_override,
                    normalized.polling_interval_seconds,
                    normalized.engine_kind,
                    normalized.enabled,
                ],
            )
            .map_err(sqlite_error)?;
        let repository_id = transaction.last_insert_rowid();

        if normalized.source_mode == SOURCE_MODE_MANAGED_REPOSITORY {
            transaction
                .execute(
                    "
                    INSERT INTO trigger_rules (
                        repository_id,
                        name,
                        source,
                        enabled,
                        config_json
                    )
                    VALUES (?, ?, ?, 1, '{}')
                    ",
                    params![
                        repository_id,
                        DEFAULT_REPOSITORY_POLL_TRIGGER_RULE_NAME,
                        TRIGGER_SOURCE_POLL,
                    ],
                )
                .map_err(sqlite_error)?;
        }

        let mut build_target_ids = Vec::with_capacity(normalized.build_targets.len());
        for target in normalized.build_targets {
            project_repository_project_build_target_contract(
                &normalized.engine_kind,
                &target.build_kind,
                &target.contract_json,
            )?;
            transaction
                .execute(
                    "
                    INSERT INTO build_targets (
                        repository_id,
                        name,
                        build_kind,
                        runner_type,
                        output_kind,
                        output_path_template,
                        timeout_seconds,
                        enabled,
                        contract_json,
                        config_json
                    )
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    ",
                    params![
                        repository_id,
                        target.name,
                        target.build_kind,
                        target.runner_type,
                        target.output_kind,
                        target.output_path_template,
                        target.timeout_seconds,
                        target.enabled,
                        target.contract_json,
                        target.runner_config_json,
                    ],
                )
                .map_err(sqlite_error)?;
            build_target_ids.push(transaction.last_insert_rowid());
        }

        let active_build_targets =
            list_active_repository_project_build_targets(&transaction, repository_id)?;
        sync_repository_project_publish_targets(
            &transaction,
            repository_id,
            normalized
                .publish_targets
                .into_iter()
                .map(promote_create_repository_project_publish_target_input)
                .collect(),
            &active_build_targets,
        )?;

        transaction.commit().map_err(sqlite_error)?;

        Ok(CreatedRepositoryProjectRecord {
            repository_id,
            repository_name: normalized.name,
            credentials_id,
            build_target_ids,
        })
    }

    /// Updates the credentials binding stored for one repository row.
    pub fn update_repository_credentials_binding(
        &self,
        repository_id: i64,
        credentials_id: Option<i64>,
    ) -> io::Result<()> {
        require_positive_identifier(repository_id, "repository id")?;

        let connection = open_connection(&self.database_path)?;
        validate_optional_credentials_binding(&connection, credentials_id)?;
        let updated = connection
            .execute(
                "
                UPDATE repositories
                SET credentials_id = ?,
                    auth_binding_status = CASE
                        WHEN auth_binding_status = 'unsupported' THEN 'unsupported'
                        WHEN auth_requirement_status = 'none' THEN 'not_required'
                        WHEN auth_requirement_status = 'required' AND ? IS NOT NULL THEN 'bound_ready'
                        WHEN auth_requirement_status = 'required' THEN 'required_unbound'
                        ELSE 'unknown'
                    END,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![credentials_id, credentials_id, repository_id],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "repository {repository_id} was not found"
            )));
        }

        Ok(())
    }

    /// Persists the latest repository access assessment snapshot for one
    /// managed repository row.
    pub fn update_repository_auth_state(
        &self,
        input: UpdateRepositoryAuthStateInput,
    ) -> io::Result<()> {
        require_positive_identifier(input.repository_id, "repository id")?;
        let source_provider_id =
            require_non_empty(&input.source_provider_id, "repository source_provider_id")?;
        let source_instance_url =
            require_non_empty(&input.source_instance_url, "repository source_instance_url")?;
        let visibility_status =
            require_non_empty(&input.visibility_status, "repository visibility_status")?;
        let auth_requirement_status = require_non_empty(
            &input.auth_requirement_status,
            "repository auth_requirement_status",
        )?;
        let auth_status_message =
            require_non_empty(&input.auth_status_message, "repository auth_status_message")?;

        let connection = open_connection(&self.database_path)?;
        let supports_interactive_login = if input.supports_interactive_login {
            1_i64
        } else {
            0_i64
        };
        let updated = connection
            .execute(
                "
                UPDATE repositories
                SET source_provider_id = ?,
                    source_instance_url = ?,
                    visibility_status = ?,
                    auth_requirement_status = ?,
                    credentials_id = CASE
                        WHEN ? = 'none' THEN NULL
                        ELSE credentials_id
                    END,
                    auth_binding_status = CASE
                        WHEN ? = 'none' THEN 'not_required'
                        WHEN ? = 'required' AND ? = 0 THEN 'unsupported'
                        WHEN ? = 'required' AND credentials_id IS NOT NULL THEN 'bound_ready'
                        WHEN ? = 'required' THEN 'required_unbound'
                        ELSE 'unknown'
                    END,
                    auth_status_message = ?,
                    auth_last_verified_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    source_provider_id,
                    source_instance_url,
                    visibility_status,
                    auth_requirement_status,
                    auth_requirement_status,
                    auth_requirement_status,
                    auth_requirement_status,
                    supports_interactive_login,
                    auth_requirement_status,
                    auth_requirement_status,
                    auth_status_message,
                    input.repository_id,
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "repository {} was not found",
                input.repository_id
            )));
        }

        Ok(())
    }

    /// Marks one repository as needing operator reauthentication after a
    /// runtime Git operation fails with an authentication error.
    pub fn mark_repository_auth_runtime_failure(
        &self,
        repository_id: i64,
        error_message: &str,
    ) -> io::Result<()> {
        require_positive_identifier(repository_id, "repository id")?;
        let error_message = require_non_empty(error_message, "error message")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE repositories
                SET auth_requirement_status = CASE
                        WHEN auth_binding_status = 'unsupported' THEN auth_requirement_status
                        ELSE 'required'
                    END,
                    auth_binding_status = CASE
                        WHEN auth_binding_status = 'unsupported' THEN 'unsupported'
                        WHEN credentials_id IS NOT NULL THEN ?
                        ELSE 'required_unbound'
                    END,
                    auth_status_message = ?,
                    auth_last_verified_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    REPOSITORY_AUTH_BINDING_STATUS_REAUTH_REQUIRED,
                    error_message,
                    repository_id,
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "repository {repository_id} was not found"
            )));
        }

        Ok(())
    }

    /// Updates the core configuration stored for one managed repository
    /// project while preserving credentials and build target registrations.
    pub fn update_repository_project(
        &self,
        input: UpdateRepositoryProjectInput,
    ) -> io::Result<()> {
        let normalized = normalize_update_repository_project_input(input)?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let source_mode = transaction
            .query_row(
                "SELECT source_mode FROM repositories WHERE id = ?",
                [normalized.repository_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)?;

        let Some(source_mode) = source_mode else {
            return Err(not_found_error(format!(
                "repository {} was not found",
                normalized.repository_id
            )));
        };

        if source_mode != normalized.source_mode {
            return Err(invalid_input_error(format!(
                "repository {} source_mode cannot change from {:?} to {:?}",
                normalized.repository_id, source_mode, normalized.source_mode
            )));
        }

        reject_duplicate_repository_project_name_for_update(
            &transaction,
            normalized.repository_id,
            &normalized.name,
        )?;
        let workspace_strategy = match normalized.source_mode.as_str() {
            SOURCE_MODE_MANAGED_REPOSITORY => {
                let repo_url = normalized.repo_url.as_deref().ok_or_else(|| {
                    invalid_input_error("repository project repo_url must not be empty")
                })?;
                reject_duplicate_repository_project_url_for_update(
                    &transaction,
                    normalized.repository_id,
                    repo_url,
                )?;
                WORKSPACE_STRATEGY_MANAGED_CHECKOUT
            }
            SOURCE_MODE_LOCAL_WORKSPACE => {
                let local_path = normalized.local_path.as_deref().ok_or_else(|| {
                    invalid_input_error("repository project local_path must not be empty")
                })?;
                reject_duplicate_repository_project_local_path_for_update(
                    &transaction,
                    normalized.repository_id,
                    local_path,
                )?;
                WORKSPACE_STRATEGY_DIRECT
            }
            _ => {
                return Err(invalid_input_error(format!(
                    "repository project source_mode {:?} is not supported",
                    normalized.source_mode
                )));
            }
        };

        transaction
            .execute(
                "
                UPDATE repositories
                SET name = ?,
                    engine_kind = ?,
                    source_mode = ?,
                    workspace_strategy = ?,
                    repo_url = ?,
                    local_path = ?,
                    default_branch = ?,
                    artifacts_root_override = ?,
                    workspace_root_override = ?,
                    polling_interval_seconds = ?,
                    enabled = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    normalized.name,
                    normalized.engine_kind,
                    normalized.source_mode,
                    workspace_strategy,
                    normalized.repo_url,
                    normalized.local_path,
                    normalized.default_branch,
                    normalized.artifacts_root_override,
                    normalized.workspace_root_override,
                    normalized.polling_interval_seconds,
                    normalized.enabled,
                    normalized.repository_id,
                ],
            )
            .map_err(sqlite_error)?;

        sync_repository_project_build_targets(
            &transaction,
            normalized.repository_id,
            &normalized.engine_kind,
            normalized.build_targets,
        )?;

        let active_build_targets =
            list_active_repository_project_build_targets(&transaction, normalized.repository_id)?;
        sync_repository_project_publish_targets(
            &transaction,
            normalized.repository_id,
            normalized.publish_targets,
            &active_build_targets,
        )?;

        transaction.commit().map_err(sqlite_error)?;

        Ok(())
    }

    /// Removes one managed repository project together with queued runtime
    /// coordination rows and returns runtime-owned paths that may be purged.
    pub fn remove_repository_project(
        &self,
        input: RemoveRepositoryProjectInput,
    ) -> io::Result<RemoveRepositoryProjectReport> {
        require_positive_identifier(input.repository_id, "repository id")?;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let repository = Self::load_repository_project_removal_record(
            &transaction,
            input.repository_id,
        )?;
        let runtime_state = Self::load_repository_project_removal_runtime_state(
            &transaction,
            input.repository_id,
            matches!(input.strategy, RemoveRepositoryProjectStrategy::Purge),
        )?;

        Self::reject_repository_project_removal_if_active(
            &transaction,
            input.repository_id,
            &repository.name,
            &runtime_state,
        )?;

        Self::delete_target_queue_messages(&transaction, &runtime_state.queue_messages)?;
        Self::delete_target_coordination_leases(
            &transaction,
            &runtime_state.coordination_lease_names,
        )?;
        Self::delete_target_idempotency_keys(
            &transaction,
            &runtime_state.idempotency_keys,
        )?;

        let deleted = transaction
            .execute(
                "DELETE FROM repositories WHERE id = ?",
                [repository.id],
            )
            .map_err(sqlite_error)?;
        if deleted == 0 {
            return Err(not_found_error(format!(
                "repository {} was not found",
                repository.id
            )));
        }

        transaction.commit().map_err(sqlite_error)?;

        Ok(RemoveRepositoryProjectReport {
            repository_id: repository.id,
            repository_name: repository.name,
            strategy: input.strategy,
            release_run_count: runtime_state.release_run_ids.len() as u64,
            build_run_count: runtime_state.build_run_ids.len() as u64,
            publish_run_count: runtime_state.publish_run_ids.len() as u64,
            queue_message_count: runtime_state.queue_messages.len() as u64,
            coordination_lease_count: runtime_state.coordination_lease_names.len() as u64,
            idempotency_key_count: runtime_state.idempotency_keys.len() as u64,
            directory_paths: runtime_state.directory_paths,
            file_paths: runtime_state.file_paths,
        })
    }

    /// Updates the credentials binding stored for one publish target row.
    pub fn update_publish_target_credentials_binding(
        &self,
        publish_target_id: i64,
        credentials_id: Option<i64>,
    ) -> io::Result<()> {
        require_positive_identifier(publish_target_id, "publish target id")?;

        let connection = open_connection(&self.database_path)?;
        validate_optional_credentials_binding(&connection, credentials_id)?;
        let updated = connection
            .execute(
                "
                UPDATE publish_targets
                SET credentials_id = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![credentials_id, publish_target_id],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "publish target {publish_target_id} was not found"
            )));
        }

        Ok(())
    }

    /// Lists repository rows needed by the runtime polling loop.
    pub fn list_polling_repositories(&self) -> io::Result<Vec<PollingRepositoryRecord>> {
        let connection = open_connection(&self.database_path)?;
        let mut statement = connection
            .prepare(
                "
                SELECT r.id,
                       r.name,
                       r.repo_url,
                      r.engine_kind,
                       r.credentials_id,
                      r.source_provider_id,
                      r.source_instance_url,
                      r.visibility_status,
                      r.auth_requirement_status,
                      r.auth_binding_status,
                      r.auth_status_message,
                      r.auth_last_verified_at,
                       r.enabled,
                       r.polling_interval_seconds,
                       r.last_seen_tag,
                       r.default_branch,
                       r.artifacts_root_override,
                       r.workspace_root_override,
                       COUNT(bt.id) AS enabled_build_target_count,
                       EXISTS(
                          SELECT 1
                          FROM release_runs rr
                          WHERE rr.repository_id = r.id
                      ) AS has_release_history
                FROM repositories r
                LEFT JOIN build_targets bt
                  ON bt.repository_id = r.id
                 AND bt.enabled = 1
                                WHERE r.source_mode = 'managed_repository'
                GROUP BY r.id, r.name, r.repo_url, r.engine_kind, r.credentials_id,
                         r.source_provider_id, r.source_instance_url,
                         r.visibility_status, r.auth_requirement_status,
                         r.auth_binding_status, r.auth_status_message,
                         r.auth_last_verified_at, r.enabled,
                         r.polling_interval_seconds, r.last_seen_tag,
                         r.default_branch, r.artifacts_root_override,
                         r.workspace_root_override
                ORDER BY r.id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(PollingRepositoryRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    repo_url: row.get(2)?,
                    engine_kind: row.get(3)?,
                    credentials_id: row.get(4)?,
                    source_provider_id: normalize_optional_string(row.get(5)?),
                    source_instance_url: normalize_optional_string(row.get(6)?),
                    visibility_status: row.get(7)?,
                    auth_requirement_status: row.get(8)?,
                    auth_binding_status: row.get(9)?,
                    auth_status_message: row.get(10)?,
                    auth_last_verified_at: normalize_optional_string(row.get(11)?),
                    enabled: row.get::<_, i64>(12)? != 0,
                    polling_interval_seconds: row.get(13)?,
                    last_seen_tag: normalize_optional_string(row.get(14)?),
                    default_branch: normalize_optional_string(row.get(15)?),
                    artifacts_root_override: normalize_optional_string(row.get(16)?),
                    workspace_root_override: normalize_optional_string(row.get(17)?),
                    enabled_build_target_count: row.get(18)?,
                    has_release_history: row.get::<_, i64>(19)? != 0,
                })
            })
            .map_err(sqlite_error)?;

        let mut repositories = Vec::new();
        for row in rows {
            repositories.push(row.map_err(sqlite_error)?);
        }

        Ok(repositories)
    }

    /// Lists repository and local-workspace project rows needed by shell
    /// inspection and project navigation.
    pub fn list_repository_projects(&self) -> io::Result<Vec<RepositoryProjectRecord>> {
        let connection = open_connection(&self.database_path)?;
        let mut statement = connection
            .prepare(
                "
                SELECT r.id,
                       r.name,
                       r.source_mode,
                       r.workspace_strategy,
                       r.repo_url,
                       r.local_path,
                       r.engine_kind,
                       r.credentials_id,
                       r.source_provider_id,
                       r.source_instance_url,
                       r.visibility_status,
                       r.auth_requirement_status,
                       r.auth_binding_status,
                       r.auth_status_message,
                       r.auth_last_verified_at,
                       r.enabled,
                       r.polling_interval_seconds,
                       r.last_seen_tag,
                       r.default_branch,
                       r.artifacts_root_override,
                       r.workspace_root_override,
                       COUNT(bt.id) AS enabled_build_target_count,
                       EXISTS(
                          SELECT 1
                          FROM release_runs rr
                          WHERE rr.repository_id = r.id
                      ) AS has_release_history
                FROM repositories r
                LEFT JOIN build_targets bt
                  ON bt.repository_id = r.id
                 AND bt.enabled = 1
                GROUP BY r.id, r.name, r.source_mode, r.workspace_strategy,
                         r.repo_url, r.local_path, r.engine_kind,
                         r.credentials_id, r.source_provider_id,
                         r.source_instance_url, r.visibility_status,
                         r.auth_requirement_status, r.auth_binding_status,
                         r.auth_status_message, r.auth_last_verified_at,
                         r.enabled, r.polling_interval_seconds,
                         r.last_seen_tag, r.default_branch,
                         r.artifacts_root_override, r.workspace_root_override
                ORDER BY r.id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                let source_mode = row.get::<_, String>(2)?;
                let polling_interval_seconds = row.get::<_, i64>(16)?;

                Ok(RepositoryProjectRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    source_mode: source_mode.clone(),
                    workspace_strategy: row.get(3)?,
                    repo_url: normalize_optional_string(row.get(4)?),
                    local_path: normalize_optional_string(row.get(5)?),
                    engine_kind: row.get(6)?,
                    credentials_id: row.get(7)?,
                    source_provider_id: normalize_optional_string(row.get(8)?),
                    source_instance_url: normalize_optional_string(row.get(9)?),
                    visibility_status: row.get(10)?,
                    auth_requirement_status: row.get(11)?,
                    auth_binding_status: row.get(12)?,
                    auth_status_message: row.get(13)?,
                    auth_last_verified_at: normalize_optional_string(row.get(14)?),
                    enabled: row.get::<_, i64>(15)? != 0,
                    polling_interval_seconds:
                        project_polling_interval_seconds_for_read_model(
                            source_mode.as_str(),
                            polling_interval_seconds,
                        ),
                    last_seen_tag: normalize_optional_string(row.get(17)?),
                    default_branch: normalize_optional_string(row.get(18)?),
                    artifacts_root_override: normalize_optional_string(row.get(19)?),
                    workspace_root_override: normalize_optional_string(row.get(20)?),
                    enabled_build_target_count: row.get(21)?,
                    has_release_history: row.get::<_, i64>(22)? != 0,
                })
            })
            .map_err(sqlite_error)?;

        let mut repositories = Vec::new();
        for row in rows {
            repositories.push(row.map_err(sqlite_error)?);
        }

        Ok(repositories)
    }

    /// Loads one repository registration row by identifier for direct checkout operations.
    pub fn get_repository_checkout_record(
        &self,
        repository_id: i64,
    ) -> io::Result<RepositoryCheckoutRecord> {
        require_positive_identifier(repository_id, "repository id")?;

        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
                       name,
                       source_mode,
                       workspace_strategy,
                       repo_url,
                      local_path,
                       credentials_id,
                       default_branch,
                       workspace_root_override,
                       enabled
                FROM repositories
                WHERE id = ?
                ",
                [repository_id],
                |row| {
                    Ok(RepositoryCheckoutRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        source_mode: row.get(2)?,
                        workspace_strategy: row.get(3)?,
                        repo_url: normalize_optional_string(row.get(4)?),
                        local_path: normalize_optional_string(row.get(5)?),
                        credentials_id: row.get(6)?,
                        default_branch: normalize_optional_string(row.get(7)?),
                        workspace_root_override: normalize_optional_string(row.get(8)?),
                        enabled: row.get::<_, i64>(9)? != 0,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    not_found_error(format!("repository {repository_id} was not found"))
                }
                other => sqlite_error(other),
            })
    }

    fn load_on_demand_repository_state(
        &self,
        repository_id: i64,
    ) -> io::Result<OnDemandRepositoryState> {
        require_positive_identifier(repository_id, "repository id")?;

        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
                       source_mode,
                       repo_url,
                       local_path,
                       credentials_id,
                       default_branch,
                       engine_kind
                FROM repositories
                WHERE id = ?
                ",
                [repository_id],
                |row| {
                    Ok(OnDemandRepositoryState {
                        repository_id: row.get(0)?,
                        source_mode: row.get::<_, String>(1)?.trim().to_owned(),
                        repo_url: normalize_optional_string(row.get(2)?),
                        local_path: normalize_optional_string(row.get(3)?),
                        credentials_id: row.get(4)?,
                        default_branch: normalize_optional_string(row.get(5)?),
                        engine_kind: parse_engine_kind_sql(6, row.get::<_, String>(6)?)?,
                    })
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    not_found_error(format!("repository {repository_id} was not found"))
                }
                other => sqlite_error(other),
            })
    }

    fn normalize_on_demand_release_dispatch_input(
        &self,
        input: OnDemandReleaseDispatchInput,
    ) -> io::Result<NormalizedOnDemandReleaseDispatchInput> {
        if input.repository_id <= 0 {
            return Err(invalid_input_error(
                "repository id must be greater than zero",
            ));
        }

        let repository = self.load_on_demand_repository_state(input.repository_id)?;
        let requested_via = input.requested_via.trim().to_owned();
        let source_kind = if input.source_kind.trim().is_empty() {
            match repository.source_mode.as_str() {
                SOURCE_MODE_LOCAL_WORKSPACE => String::from(RELEASE_SOURCE_KIND_LOCAL_WORKSPACE),
                _ => {
                    return Err(invalid_input_error(
                        "on-demand release source_kind must not be empty",
                    ));
                }
            }
        } else {
            normalize_release_source_kind(&input.source_kind)?
        };
        let version_source = normalize_release_version_source(&input.version_source)?;
        let mut source_ref = normalize_optional_string(input.source_ref);
        let mut local_path = normalize_absolute_path_string(
            input.local_path,
            "on-demand release local_path",
        )?;

        match source_kind.as_str() {
            RELEASE_SOURCE_KIND_MANAGED_TAG | RELEASE_SOURCE_KIND_MANAGED_REF => {
                if repository.repo_url.is_none() {
                    return Err(invalid_input_error(format!(
                        "repository {} does not define a managed repo_url for source_kind {:?}",
                        repository.repository_id, source_kind
                    )));
                }

                if source_ref.is_none() {
                    source_ref = repository.default_branch.clone();
                }
                source_ref = Some(require_non_empty(
                    source_ref.as_deref().unwrap_or_default(),
                    "on-demand release source_ref",
                )?);
            }
            RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => {
                if local_path.is_none() {
                    local_path = repository.local_path.clone();
                }
                if local_path.is_none() {
                    return Err(invalid_input_error(format!(
                        "repository {} does not define a local workspace path for on-demand execution",
                        repository.repository_id
                    )));
                }
            }
            _ => {
                return Err(invalid_input_error(format!(
                    "unsupported on-demand release source_kind {:?}",
                    source_kind
                )));
            }
        }

        let git_tag = match version_source.as_str() {
            RELEASE_VERSION_SOURCE_MANUAL => require_non_empty(
                input.release_version.as_deref().unwrap_or_default(),
                "on-demand release version",
            )?,
            RELEASE_VERSION_SOURCE_PROJECT_SETTINGS => self.resolve_on_demand_release_version(
                &repository,
                &source_kind,
                source_ref.as_deref(),
                local_path.as_deref(),
            )?,
            other => {
                return Err(invalid_input_error(format!(
                    "unsupported on-demand release version_source {:?}",
                    other
                )));
            }
        };
        let unity_executable_path_override = normalize_absolute_path_string(
            input.unity_executable_path_override,
            "unity executable path override",
        )?;
        let source_identity = build_release_source_identity(
            &source_kind,
            source_ref.as_deref(),
            local_path.as_deref(),
        )?;

        Ok(NormalizedOnDemandReleaseDispatchInput {
            repository_id: input.repository_id,
            git_tag,
            git_commit: String::new(),
            requested_via,
            source_kind,
            source_ref,
            local_path,
            version_source,
            unity_executable_path_override,
            source_identity,
        })
    }

    fn resolve_on_demand_release_version(
        &self,
        repository: &OnDemandRepositoryState,
        source_kind: &str,
        source_ref: Option<&str>,
        local_path: Option<&str>,
    ) -> io::Result<String> {
        match source_kind {
            RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => detect_release_version_from_local_workspace(
                repository.engine_kind,
                local_path.ok_or_else(|| {
                    invalid_input_error("on-demand local workspace path must not be empty")
                })?,
            ),
            RELEASE_SOURCE_KIND_MANAGED_TAG | RELEASE_SOURCE_KIND_MANAGED_REF => {
                let repository_url = repository.repo_url.as_deref().ok_or_else(|| {
                    invalid_input_error("managed repository URL must not be empty")
                })?;
                let git_ref = source_ref.ok_or_else(|| {
                    invalid_input_error("on-demand source_ref must not be empty")
                })?;
                let git_auth = self.resolve_release_git_auth(repository.credentials_id)?;

                detect_release_version_from_git_ref(
                    repository.engine_kind,
                    repository_url,
                    git_ref,
                    &git_auth,
                )
            }
            other => Err(invalid_input_error(format!(
                "unsupported on-demand release source_kind {:?}",
                other
            ))),
        }
    }

    /// Imports one repository registration and its configuration from another runtime database.
    pub fn import_repository_registration_from_database(
        &self,
        source_database_path: &Path,
        repository_name: &str,
    ) -> io::Result<ImportedRepositoryRegistrationReport> {
        let repository_name = require_non_empty(repository_name, "repository name")?;
        if source_database_path.as_os_str().is_empty() {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "source database path must not be empty",
            ));
        }
        if !source_database_path.is_file() {
            return Err(not_found_error(format!(
                "source database {:?} was not found",
                source_database_path
            )));
        }

        let source_database_path = source_database_path
            .canonicalize()
            .unwrap_or_else(|_| source_database_path.to_path_buf());
        let mut connection = open_connection(&self.database_path)?;
        connection
            .execute(
                "ATTACH DATABASE ? AS source_db",
                params![source_database_path.display().to_string()],
            )
            .map_err(sqlite_error)?;

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                SELECT DISTINCT c.name,
                                c.kind,
                                c.config_json
                FROM source_db.credentials c
                WHERE c.id IN (
                    SELECT r.credentials_id
                    FROM source_db.repositories r
                    WHERE r.name = ?
                      AND r.credentials_id IS NOT NULL

                    UNION

                    SELECT pt.credentials_id
                    FROM source_db.publish_targets pt
                    JOIN source_db.repositories r ON r.id = pt.repository_id
                    WHERE r.name = ?
                      AND pt.credentials_id IS NOT NULL
                )
                ON CONFLICT(name) DO UPDATE SET
                    kind = excluded.kind,
                    config_json = excluded.config_json,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![repository_name, repository_name],
            )
            .map_err(sqlite_error)?;

        let imported = transaction
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    source_mode,
                    workspace_strategy,
                    repo_url,
                    local_path,
                    credentials_id,
                    default_branch,
                    artifacts_root_override,
                    workspace_root_override,
                    polling_interval_seconds,
                    last_seen_tag,
                    enabled
                )
                SELECT r.name,
                       r.source_mode,
                       r.workspace_strategy,
                       r.repo_url,
                       r.local_path,
                       (
                           SELECT tc.id
                           FROM credentials tc
                           JOIN source_db.credentials sc ON sc.name = tc.name
                           WHERE sc.id = r.credentials_id
                       ),
                       r.default_branch,
                       r.artifacts_root_override,
                       r.workspace_root_override,
                       r.polling_interval_seconds,
                       r.last_seen_tag,
                       r.enabled
                FROM source_db.repositories r
                WHERE r.name = ?
                ON CONFLICT(name) DO UPDATE SET
                    source_mode = excluded.source_mode,
                    workspace_strategy = excluded.workspace_strategy,
                    repo_url = excluded.repo_url,
                    local_path = excluded.local_path,
                    credentials_id = excluded.credentials_id,
                    default_branch = excluded.default_branch,
                    artifacts_root_override = excluded.artifacts_root_override,
                    workspace_root_override = excluded.workspace_root_override,
                    polling_interval_seconds = excluded.polling_interval_seconds,
                    last_seen_tag = excluded.last_seen_tag,
                    enabled = excluded.enabled,
                    updated_at = CURRENT_TIMESTAMP
                ",
                [repository_name.as_str()],
            )
            .map_err(sqlite_error)?;
        if imported == 0 {
            return Err(not_found_error(format!(
                "repository registration {repository_name:?} was not found in {:?}",
                source_database_path
            )));
        }

        let repository_id: i64 = transaction
            .query_row(
                "SELECT id FROM repositories WHERE name = ?",
                [repository_name.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "DELETE FROM release_runs WHERE repository_id = ?",
                [repository_id],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM trigger_rules WHERE repository_id = ?",
                [repository_id],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM build_targets WHERE repository_id = ?",
                [repository_id],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "DELETE FROM publish_targets WHERE repository_id = ?",
                [repository_id],
            )
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                INSERT INTO trigger_rules (
                    repository_id,
                    name,
                    source,
                    enabled,
                    config_json
                )
                SELECT ?,
                       t.name,
                       t.source,
                       t.enabled,
                       t.config_json
                FROM source_db.trigger_rules t
                JOIN source_db.repositories r ON r.id = t.repository_id
                WHERE r.name = ?
                ",
                params![repository_id, repository_name],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    output_kind,
                    output_path_template,
                    timeout_seconds,
                    enabled,
                    contract_json,
                    config_json
                )
                SELECT ?,
                       bt.name,
                       bt.build_kind,
                       bt.runner_type,
                       bt.output_kind,
                       bt.output_path_template,
                       bt.timeout_seconds,
                       bt.enabled,
                       bt.contract_json,
                       bt.config_json
                FROM source_db.build_targets bt
                JOIN source_db.repositories r ON r.id = bt.repository_id
                WHERE r.name = ?
                ",
                params![repository_id, repository_name],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    credentials_id,
                    enabled,
                    config_json
                )
                SELECT ?,
                       pt.name,
                       pt.kind,
                       (
                           SELECT tc.id
                           FROM credentials tc
                           JOIN source_db.credentials sc ON sc.name = tc.name
                           WHERE sc.id = pt.credentials_id
                       ),
                       pt.enabled,
                       pt.config_json
                FROM source_db.publish_targets pt
                JOIN source_db.repositories r ON r.id = pt.repository_id
                WHERE r.name = ?
                ",
                params![repository_id, repository_name],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "
                INSERT INTO build_publish_bindings (
                    build_target_id,
                    publish_target_id,
                    enabled,
                    options_json
                )
                SELECT tb.id,
                       tp.id,
                       bpb.enabled,
                       bpb.options_json
                FROM source_db.build_publish_bindings bpb
                JOIN source_db.build_targets sbt ON sbt.id = bpb.build_target_id
                JOIN source_db.publish_targets spt ON spt.id = bpb.publish_target_id
                JOIN source_db.repositories sr ON sr.id = sbt.repository_id
                JOIN build_targets tb ON tb.repository_id = ? AND tb.name = sbt.name
                JOIN publish_targets tp ON tp.repository_id = ? AND tp.name = spt.name
                WHERE sr.name = ?
                ",
                params![repository_id, repository_id, repository_name],
            )
            .map_err(sqlite_error)?;

        let credential_name = transaction
            .query_row(
                "
                SELECT c.name
                FROM repositories r
                JOIN credentials c ON c.id = r.credentials_id
                WHERE r.id = ?
                ",
                [repository_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)?;
        let trigger_rule_count = transaction
            .query_row(
                "SELECT COUNT(1) FROM trigger_rules WHERE repository_id = ?",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        let build_target_count = transaction
            .query_row(
                "SELECT COUNT(1) FROM build_targets WHERE repository_id = ?",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        let publish_target_count = transaction
            .query_row(
                "SELECT COUNT(1) FROM publish_targets WHERE repository_id = ?",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        let binding_count = transaction
            .query_row(
                "
                SELECT COUNT(1)
                FROM build_publish_bindings
                WHERE build_target_id IN (
                    SELECT id FROM build_targets WHERE repository_id = ?
                )
                ",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        transaction.commit().map_err(sqlite_error)?;

        Ok(ImportedRepositoryRegistrationReport {
            source_database_path: source_database_path.display().to_string(),
            repository_id,
            repository_name,
            credential_name,
            trigger_rule_count,
            build_target_count,
            publish_target_count,
            binding_count,
        })
    }

    /// Updates the durable last-seen Git tag baseline for one repository.
    pub fn update_repository_last_seen_tag(
        &self,
        repository_id: i64,
        git_tag: &str,
    ) -> io::Result<()> {
        require_positive_identifier(repository_id, "repository id")?;
        let git_tag = require_non_empty(git_tag, "git tag")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE repositories
                SET last_seen_tag = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![git_tag, repository_id],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "repository {repository_id} was not found"
            )));
        }

        Ok(())
    }

    /// Claims one queued build run into the running state and persists execution paths.
    pub fn start_build_run(
        &self,
        build_run_id: i64,
        input: StartBuildRunInput,
    ) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    workspace_path = COALESCE(?, workspace_path),
                    log_path = COALESCE(?, log_path),
                    artifact_root_path = COALESCE(?, artifact_root_path),
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    finished_at = NULL,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    BuildStatus::Running.as_str(),
                    nullable_string(&input.workspace_path),
                    nullable_string(&input.log_path),
                    nullable_string(&input.artifact_root_path),
                    build_run_id,
                    BuildStatus::Queued.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Queued.as_str(),
                "start",
            ));
        }

        let record = self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "started build run {build_run_id} could not be reloaded"
            ))
        })?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Marks one running build run as completed and clears any stored error.
    pub fn complete_build_run(
        &self,
        build_run_id: i64,
        input: CompleteBuildRunInput,
    ) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    workspace_path = COALESCE(?, workspace_path),
                    log_path = COALESCE(?, log_path),
                    artifact_root_path = COALESCE(?, artifact_root_path),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    BuildStatus::Succeeded.as_str(),
                    nullable_string(&input.workspace_path),
                    nullable_string(&input.log_path),
                    nullable_string(&input.artifact_root_path),
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "complete",
            ));
        }

        let record = self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "completed build run {build_run_id} could not be reloaded"
            ))
        })?;
        self.advance_release_after_terminal_build(record.release_run_id)?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Marks one running build run as failed and stores the terminal error message.
    pub fn fail_build_run(
        &self,
        build_run_id: i64,
        input: FailBuildRunInput,
    ) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;
        let error_message = require_non_empty(&input.error_message, "error message")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    workspace_path = COALESCE(?, workspace_path),
                    log_path = COALESCE(?, log_path),
                    artifact_root_path = COALESCE(?, artifact_root_path),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    BuildStatus::Failed.as_str(),
                    nullable_string(&input.workspace_path),
                    nullable_string(&input.log_path),
                    nullable_string(&input.artifact_root_path),
                    error_message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "fail",
            ));
        }

        let record = self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "failed build run {build_run_id} could not be reloaded"
            ))
        })?;
        self.advance_release_after_terminal_build(record.release_run_id)?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Marks one running build run as canceled and stores the terminal reason.
    pub fn cancel_build_run(
        &self,
        build_run_id: i64,
        input: CancelBuildRunInput,
    ) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;
        let error_message = require_non_empty(&input.error_message, "error message")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    workspace_path = COALESCE(?, workspace_path),
                    log_path = COALESCE(?, log_path),
                    artifact_root_path = COALESCE(?, artifact_root_path),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    BuildStatus::Canceled.as_str(),
                    nullable_string(&input.workspace_path),
                    nullable_string(&input.log_path),
                    nullable_string(&input.artifact_root_path),
                    error_message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "cancel",
            ));
        }

        let record = self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "canceled build run {build_run_id} could not be reloaded"
            ))
        })?;
        self.advance_release_after_terminal_build(record.release_run_id)?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Cancels one release process and all queued or running child runs.
    pub fn cancel_release_run(
        &self,
        release_run_id: i64,
        input: CancelReleaseRunInput,
    ) -> io::Result<ReleaseRunRecord> {
        require_positive_identifier(release_run_id, "release run id")?;
        let error_message = require_non_empty(&input.error_message, "error message")?;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let release = self
            .load_release_run_record_in_transaction(&transaction, release_run_id)?
            .ok_or_else(|| {
                not_found_error(format!("release run {release_run_id} was not found"))
            })?;
        if release.status == ReleaseStatus::Succeeded.as_str()
            || release.status == ReleaseStatus::Failed.as_str()
            || release.status == ReleaseStatus::Canceled.as_str()
        {
            transaction.commit().map_err(sqlite_error)?;
            return self.get_release_run_record(release_run_id);
        }

        transaction
            .execute(
                "
                UPDATE release_runs
                SET status = ?,
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    ReleaseStatus::Canceled.as_str(),
                    error_message,
                    release_run_id,
                ],
            )
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                UPDATE build_runs
                SET status = ?,
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = CASE
                        WHEN error_message IS NULL OR TRIM(error_message) = '' THEN ?
                        ELSE error_message
                    END,
                    updated_at = CURRENT_TIMESTAMP
                WHERE release_run_id = ?
                  AND status IN (?, ?)
                ",
                params![
                    BuildStatus::Canceled.as_str(),
                    error_message,
                    release_run_id,
                    BuildStatus::Queued.as_str(),
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "
                UPDATE publish_runs
                SET status = ?,
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = CASE
                        WHEN error_message IS NULL OR TRIM(error_message) = '' THEN ?
                        ELSE error_message
                    END,
                    updated_at = CURRENT_TIMESTAMP
                WHERE release_run_id = ?
                  AND status IN (?, ?)
                ",
                params![
                    PublishStatus::Canceled.as_str(),
                    error_message,
                    release_run_id,
                    PublishStatus::Queued.as_str(),
                    PublishStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;

        transaction.commit().map_err(sqlite_error)?;

        self.reconcile_release_run_status(release_run_id)?;
        self.get_release_run_record(release_run_id)
    }

    /// Starts or refreshes one named stage under a running build run.
    pub fn start_build_run_stage(
        &self,
        build_run_id: i64,
        input: StartBuildRunStageInput,
    ) -> io::Result<BuildRunStageRecord> {
        require_positive_identifier(build_run_id, "build run id")?;
        if input.position < 0 {
            return Err(invalid_input_error(
                "build run stage position must not be negative",
            ));
        }

        let step_key = require_non_empty(&input.step_key, "build run stage key")?;
        let step_label = require_non_empty(&input.step_label, "build run stage label")?;
        let step_log_path = require_non_empty(&input.step_log_path, "build run stage log path")?;
        let workspace_path = require_non_empty(&input.workspace_path, "build run workspace path")?;
        let log_path = require_non_empty(&input.log_path, "build run log path")?;
        let artifact_root_path =
            require_non_empty(&input.artifact_root_path, "build run artifact root path")?;
        let message = require_non_empty(&input.message, "build run stage message")?;

        let connection = open_connection(&self.database_path)?;
        let build_updated = connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    current_stage_key = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    last_progress_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    step_key,
                    step_label,
                    BuildStatus::Running.as_str(),
                    message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if build_updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "start build stage",
            ));
        }

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
                ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, NULL, NULL, CURRENT_TIMESTAMP)
                ON CONFLICT(build_run_id, step_key) DO UPDATE SET
                    position = excluded.position,
                    step_label = excluded.step_label,
                    status = excluded.status,
                    log_path = excluded.log_path,
                    last_message = excluded.last_message,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    started_at = COALESCE(build_run_steps.started_at, CURRENT_TIMESTAMP),
                    finished_at = NULL,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                ",
                params![
                    build_run_id,
                    input.position,
                    step_key,
                    step_label,
                    BuildStatus::Running.as_str(),
                    step_log_path,
                    message,
                ],
            )
            .map_err(sqlite_error)?;

        self.load_build_run_stage_record(build_run_id, step_key.as_str())?.ok_or_else(|| {
            not_found_error(format!(
                "started build stage {:?} for build run {build_run_id} could not be reloaded",
                step_key,
            ))
        })
    }

    /// Refreshes the heartbeat of one running build stage and the parent build run.
    pub fn heartbeat_build_run_stage(
        &self,
        build_run_id: i64,
        input: HeartbeatBuildRunStageInput,
    ) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        let step_key = require_non_empty(&input.step_key, "build run stage key")?;
        let step_label = require_non_empty(&input.step_label, "build run stage label")?;
        let step_log_path = require_non_empty(&input.step_log_path, "build run stage log path")?;
        let workspace_path = require_non_empty(&input.workspace_path, "build run workspace path")?;
        let log_path = require_non_empty(&input.log_path, "build run log path")?;
        let artifact_root_path =
            require_non_empty(&input.artifact_root_path, "build run artifact root path")?;
        let message = require_non_empty(&input.message, "build run stage message")?;

        let connection = open_connection(&self.database_path)?;
        let stage_updated = connection
            .execute(
                "
                UPDATE build_run_steps
                SET step_label = ?,
                    log_path = ?,
                    status = ?,
                    last_message = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    updated_at = CURRENT_TIMESTAMP
                WHERE build_run_id = ?
                  AND step_key = ?
                ",
                params![
                    step_label,
                    step_log_path,
                    BuildStatus::Running.as_str(),
                    message,
                    build_run_id,
                    step_key,
                ],
            )
            .map_err(sqlite_error)?;
        if stage_updated == 0 {
            return Err(not_found_error(format!(
                "build stage {:?} for build run {build_run_id} was not found",
                step_key,
            )));
        }

        let build_updated = connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    current_stage_key = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    last_progress_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    step_key,
                    step_label,
                    BuildStatus::Running.as_str(),
                    message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if build_updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "heartbeat build stage",
            ));
        }

        self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "heartbeated build run {build_run_id} could not be reloaded"
            ))
        })
    }

    /// Completes one build stage and keeps the parent build run marked as running.
    pub fn complete_build_run_stage(
        &self,
        build_run_id: i64,
        input: CompleteBuildRunStageInput,
    ) -> io::Result<BuildRunStageRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        let step_key = require_non_empty(&input.step_key, "build run stage key")?;
        let step_label = require_non_empty(&input.step_label, "build run stage label")?;
        let step_log_path = require_non_empty(&input.step_log_path, "build run stage log path")?;
        let workspace_path = require_non_empty(&input.workspace_path, "build run workspace path")?;
        let log_path = require_non_empty(&input.log_path, "build run log path")?;
        let artifact_root_path =
            require_non_empty(&input.artifact_root_path, "build run artifact root path")?;
        let message = require_non_empty(&input.message, "build run stage message")?;

        let connection = open_connection(&self.database_path)?;
        let stage_updated = connection
            .execute(
                "
                UPDATE build_run_steps
                SET step_label = ?,
                    log_path = ?,
                    status = ?,
                    last_message = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE build_run_id = ?
                  AND step_key = ?
                ",
                params![
                    step_label,
                    step_log_path,
                    BuildStatus::Succeeded.as_str(),
                    message,
                    build_run_id,
                    step_key,
                ],
            )
            .map_err(sqlite_error)?;
        if stage_updated == 0 {
            return Err(not_found_error(format!(
                "build stage {:?} for build run {build_run_id} was not found",
                step_key,
            )));
        }

        let build_updated = connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    current_stage_key = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    last_progress_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    step_key,
                    step_label,
                    BuildStatus::Succeeded.as_str(),
                    message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if build_updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "complete build stage",
            ));
        }

        self.load_build_run_stage_record(build_run_id, step_key.as_str())?.ok_or_else(|| {
            not_found_error(format!(
                "completed build stage {:?} for build run {build_run_id} could not be reloaded",
                step_key,
            ))
        })
    }

    /// Fails one build stage while keeping the parent build run transition separate.
    pub fn fail_build_run_stage(
        &self,
        build_run_id: i64,
        input: FailBuildRunStageInput,
    ) -> io::Result<BuildRunStageRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        let step_key = require_non_empty(&input.step_key, "build run stage key")?;
        let step_label = require_non_empty(&input.step_label, "build run stage label")?;
        let step_log_path = require_non_empty(&input.step_log_path, "build run stage log path")?;
        let workspace_path = require_non_empty(&input.workspace_path, "build run workspace path")?;
        let log_path = require_non_empty(&input.log_path, "build run log path")?;
        let artifact_root_path =
            require_non_empty(&input.artifact_root_path, "build run artifact root path")?;
        let error_message = require_non_empty(&input.error_message, "build run stage error message")?;

        let connection = open_connection(&self.database_path)?;
        let stage_updated = connection
            .execute(
                "
                UPDATE build_run_steps
                SET step_label = ?,
                    log_path = ?,
                    status = ?,
                    last_message = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE build_run_id = ?
                  AND step_key = ?
                ",
                params![
                    step_label,
                    step_log_path,
                    BuildStatus::Failed.as_str(),
                    error_message,
                    error_message,
                    build_run_id,
                    step_key,
                ],
            )
            .map_err(sqlite_error)?;
        if stage_updated == 0 {
            return Err(not_found_error(format!(
                "build stage {:?} for build run {build_run_id} was not found",
                step_key,
            )));
        }

        let build_updated = connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    current_stage_key = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    heartbeat_at = CURRENT_TIMESTAMP,
                    last_progress_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    workspace_path,
                    log_path,
                    artifact_root_path,
                    step_key,
                    step_label,
                    BuildStatus::Failed.as_str(),
                    error_message,
                    build_run_id,
                    BuildStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if build_updated == 0 {
            return Err(self.build_run_transition_error(
                build_run_id,
                BuildStatus::Running.as_str(),
                "fail build stage",
            ));
        }

        self.load_build_run_stage_record(build_run_id, step_key.as_str())?.ok_or_else(|| {
            not_found_error(format!(
                "failed build stage {:?} for build run {build_run_id} could not be reloaded",
                step_key,
            ))
        })
    }

    /// Lists the durable stages recorded for one build run in execution order.
    pub fn list_build_run_stages(
        &self,
        build_run_id: i64,
    ) -> io::Result<Vec<BuildRunStageRecord>> {
        require_positive_identifier(build_run_id, "build run id")?;

        let connection = open_connection(&self.database_path)?;
        let mut statement = connection
            .prepare(
                "
                SELECT id,
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
                       created_at,
                       updated_at
                FROM build_run_steps
                WHERE build_run_id = ?
                ORDER BY position ASC, id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([build_run_id], scan_build_run_stage_record)
            .map_err(sqlite_error)?;

        let mut stages = Vec::new();
        for row in rows {
            stages.push(row.map_err(sqlite_error)?);
        }

        Ok(stages)
    }

    /// Replaces the durable artifact set recorded for one build run in a single transaction.
    pub fn replace_build_artifacts(
        &self,
        build_run_id: i64,
        inputs: Vec<CreateArtifactRecordInput>,
    ) -> io::Result<Vec<ArtifactRecord>> {
        require_positive_identifier(build_run_id, "build run id")?;
        self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!("build run {build_run_id} was not found"))
        })?;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "DELETE FROM artifacts WHERE build_run_id = ?",
                [build_run_id],
            )
            .map_err(sqlite_error)?;

        for input in inputs {
            let normalized = normalize_artifact_record_input(input)?;
            transaction
                .execute(
                    "
                    INSERT INTO artifacts (
                        build_run_id,
                        name,
                        kind,
                        path,
                        active_location_kind,
                        active_location_ref,
                        size_bytes,
                        checksum_sha256
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    ",
                    params![
                        build_run_id,
                        normalized.name,
                        normalized.kind,
                        normalized.path,
                        "runtime_artifact",
                        normalized.path,
                        normalized.size_bytes,
                        normalized.checksum_sha256,
                    ],
                )
                .map_err(sqlite_error)?;
        }

        let artifacts = list_build_artifacts_with_connection(&transaction, build_run_id)?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(artifacts)
    }

    /// Lists the durable artifact metadata currently registered for one build run.
    pub fn list_artifacts_by_build_run(
        &self,
        build_run_id: i64,
    ) -> io::Result<Vec<ArtifactRecord>> {
        require_positive_identifier(build_run_id, "build run id")?;
        self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!("build run {build_run_id} was not found"))
        })?;

        let connection = open_connection(&self.database_path)?;
        list_build_artifacts_with_connection(&connection, build_run_id)
    }

    /// Materializes queued publish runs for every enabled binding and registered artifact.
    pub fn plan_build_publish_runs(
        &self,
        build_run_id: i64,
    ) -> io::Result<Vec<PublishRunRecord>> {
        require_positive_identifier(build_run_id, "build run id")?;

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let Some(summary) = load_build_publish_summary(&transaction, build_run_id)? else {
            return Err(not_found_error(format!(
                "build run {build_run_id} was not found"
            )));
        };

        let artifact_ids = list_build_artifact_ids(&transaction, build_run_id)?;
        if artifact_ids.is_empty() {
            return Err(invalid_input_error(format!(
                "build run {build_run_id} has no registered artifacts"
            )));
        }

        let publish_bindings =
            list_enabled_publish_binding_plans(&transaction, summary.build_target_id)?;
        let mut existing = list_existing_publish_run_keys(&transaction, build_run_id)?;

        for artifact_id in &artifact_ids {
            for publish_binding in &publish_bindings {
                let publish_target_id = publish_binding.publish_target_id;
                if existing.contains(&(publish_target_id, *artifact_id)) {
                    continue;
                }

                let execution_contract_json = build_publish_run_execution_contract_json(
                    &transaction,
                    publish_binding,
                    *artifact_id,
                )?;

                transaction
                    .execute(
                        "
                        INSERT INTO publish_runs (
                            release_run_id,
                            build_run_id,
                            publish_target_id,
                            artifact_id,
                            status,
                            execution_contract_json
                        ) VALUES (?, ?, ?, ?, ?, ?)
                        ",
                        params![
                            summary.release_run_id,
                            build_run_id,
                            publish_target_id,
                            artifact_id,
                            PublishStatus::Queued.as_str(),
                            execution_contract_json,
                        ],
                    )
                    .map_err(sqlite_error)?;
                existing.insert((publish_target_id, *artifact_id));
            }
        }

        let runs = list_publish_runs_with_connection(&transaction, build_run_id)?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(runs)
    }

    /// Lists the durable publish runs created for one build result.
    pub fn list_publish_runs_by_build_run(
        &self,
        build_run_id: i64,
    ) -> io::Result<Vec<PublishRunRecord>> {
        require_positive_identifier(build_run_id, "build run id")?;
        self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!("build run {build_run_id} was not found"))
        })?;

        let connection = open_connection(&self.database_path)?;
        list_publish_runs_with_connection(&connection, build_run_id)
    }

    /// Loads one durable build run by identifier.
    pub fn get_build_run_record(&self, build_run_id: i64) -> io::Result<BuildRunRecord> {
        require_positive_identifier(build_run_id, "build run id")?;

        self.load_build_run_record(build_run_id)?.ok_or_else(|| {
            not_found_error(format!("build run {build_run_id} was not found"))
        })
    }

    /// Loads one durable publish run by identifier.
    pub fn get_publish_run_record(&self, publish_run_id: i64) -> io::Result<PublishRunRecord> {
        require_positive_identifier(publish_run_id, "publish run id")?;

        self.load_publish_run_record(publish_run_id)?.ok_or_else(|| {
            not_found_error(format!("publish run {publish_run_id} was not found"))
        })
    }

    /// Loads the joined metadata required to execute one publish run.
    pub fn get_publish_execution_plan(
        &self,
        publish_run_id: i64,
    ) -> io::Result<PublishExecutionPlan> {
        require_positive_identifier(publish_run_id, "publish run id")?;

        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT pr.id,
                       pr.release_run_id,
                       rr.repository_id,
                       r.name,
                       rr.git_tag,
                       pr.build_run_id,
                       pr.publish_target_id,
                       pt.name,
                       pt.kind,
                       pt.config_json,
                      pr.execution_contract_json,
                       pr.status,
                       pr.artifact_id,
                       a.name,
                       a.kind,
                       a.path,
                      br.artifact_root_path,
                      a.active_location_kind,
                      a.active_location_ref
                FROM publish_runs pr
                JOIN release_runs rr ON rr.id = pr.release_run_id
                JOIN repositories r ON r.id = rr.repository_id
                JOIN publish_targets pt ON pt.id = pr.publish_target_id
                JOIN build_runs br ON br.id = pr.build_run_id
                LEFT JOIN artifacts a ON a.id = pr.artifact_id
                WHERE pr.id = ?
                ",
                [publish_run_id],
                scan_publish_execution_plan,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    not_found_error(format!("publish run {publish_run_id} was not found"))
                }
                other => sqlite_error(other),
            })
    }

    /// Claims one queued publish run into the running state.
    pub fn start_publish_run(
        &self,
        publish_run_id: i64,
        _input: StartPublishRunInput,
    ) -> io::Result<PublishRunRecord> {
        require_positive_identifier(publish_run_id, "publish run id")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE publish_runs
                SET status = ?,
                    started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
                    finished_at = NULL,
                    destination_ref = NULL,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    PublishStatus::Running.as_str(),
                    publish_run_id,
                    PublishStatus::Queued.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.publish_run_transition_error(
                publish_run_id,
                PublishStatus::Queued.as_str(),
                "start",
            ));
        }

        let record = self.load_publish_run_record(publish_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "started publish run {publish_run_id} could not be reloaded"
            ))
        })?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Marks one running publish run as succeeded.
    pub fn complete_publish_run(
        &self,
        publish_run_id: i64,
        input: CompletePublishRunInput,
    ) -> io::Result<PublishRunRecord> {
        require_positive_identifier(publish_run_id, "publish run id")?;
        if input.artifact_active_location_kind.is_some()
            != input.artifact_active_location_ref.is_some()
        {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "artifact active location updates require both kind and ref",
            ));
        }

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE publish_runs
                SET status = ?,
                    destination_ref = COALESCE(?, destination_ref),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    PublishStatus::Succeeded.as_str(),
                    nullable_string(&input.destination_ref),
                    publish_run_id,
                    PublishStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.publish_run_transition_error(
                publish_run_id,
                PublishStatus::Running.as_str(),
                "complete",
            ));
        }

        if let (Some(active_location_kind), Some(active_location_ref)) = (
            normalize_optional_string(input.artifact_active_location_kind.clone()),
            normalize_optional_string(input.artifact_active_location_ref.clone()),
        ) {
            let updated_artifacts = connection
                .execute(
                    "
                    UPDATE artifacts
                    SET active_location_kind = ?,
                        active_location_ref = ?
                    WHERE id = (
                        SELECT artifact_id
                        FROM publish_runs
                        WHERE id = ?
                    )
                    ",
                    params![active_location_kind, active_location_ref, publish_run_id],
                )
                .map_err(sqlite_error)?;
            if updated_artifacts == 0 {
                return Err(not_found_error(format!(
                    "publish run {publish_run_id} does not reference an artifact to update"
                )));
            }
        }

        let record = self.load_publish_run_record(publish_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "completed publish run {publish_run_id} could not be reloaded"
            ))
        })?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Marks one running publish run as failed and stores the terminal error message.
    pub fn fail_publish_run(
        &self,
        publish_run_id: i64,
        input: FailPublishRunInput,
    ) -> io::Result<PublishRunRecord> {
        require_positive_identifier(publish_run_id, "publish run id")?;
        let error_message = require_non_empty(&input.error_message, "error message")?;

        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE publish_runs
                SET status = ?,
                    destination_ref = COALESCE(?, destination_ref),
                    finished_at = CURRENT_TIMESTAMP,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND status = ?
                ",
                params![
                    PublishStatus::Failed.as_str(),
                    nullable_string(&input.destination_ref),
                    error_message,
                    publish_run_id,
                    PublishStatus::Running.as_str(),
                ],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(self.publish_run_transition_error(
                publish_run_id,
                PublishStatus::Running.as_str(),
                "fail",
            ));
        }

        let record = self.load_publish_run_record(publish_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "failed publish run {publish_run_id} could not be reloaded"
            ))
        })?;
        self.reconcile_release_run_status(record.release_run_id)?;

        Ok(record)
    }

    /// Claims the next eligible release job from the local queue for repository-scoped planning.
    pub fn claim_next_release_job(
        &self,
        worker_name: &str,
        wait: Duration,
        lease_ttl: Duration,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let worker_name = require_non_empty(worker_name, "worker name")?;
        let lease_ttl_millis = duration_to_millis(lease_ttl, "queue lease ttl")?;
        let started_at = Instant::now();

        loop {
            if let Some(message) = self.claim_next_release_job_once(&worker_name, lease_ttl_millis)?
            {
                return Ok(Some(message));
            }

            if started_at.elapsed() >= wait {
                return Ok(None);
            }

            thread::sleep(next_poll_interval(wait, started_at.elapsed()));
        }
    }

    /// Processes one queued release message and advances repository-local release planning.
    pub fn process_next_release_job(
        &self,
        worker_name: &str,
        wait: Duration,
        lease_ttl: Duration,
    ) -> io::Result<bool> {
        let Some(message) = self.claim_next_release_job(worker_name, wait, lease_ttl)? else {
            return Ok(false);
        };
        let job = decode_release_dispatch_job(&message.payload)?;

        let disposition = match self.process_claimed_release_job(&job) {
            Ok(disposition) => disposition,
            Err(error) => {
                let _ = self.release_message(message.id, &message.lease_token);
                return Err(error);
            }
        };

        match disposition {
            ReleaseJobDisposition::Acknowledge => {
                let acknowledged = self.acknowledge_message(message.id, &message.lease_token)?;
                if !acknowledged {
                    return Err(io::Error::new(
                        ErrorKind::NotFound,
                        format!("release queue message {} could not be acknowledged", message.id),
                    ));
                }
            }
            ReleaseJobDisposition::RetryLater => {
                let released = self.release_message(message.id, &message.lease_token)?;
                if !released {
                    return Err(io::Error::new(
                        ErrorKind::NotFound,
                        format!("release queue message {} could not be released", message.id),
                    ));
                }
            }
        }

        Ok(true)
    }

    /// Returns one read-only snapshot of the local automation queues, leases, and repository backlog.
    pub fn automation_snapshot(&self) -> io::Result<AutomationSnapshot> {
        self.reconcile_release_run_statuses()?;

        let now = unix_timestamp_millis()?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(sqlite_error)?;
        let generated_at = transaction
            .query_row(
                "SELECT STRFTIME('%Y-%m-%dT%H:%M:%SZ', 'now')",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(sqlite_error)?;

        let queue_messages = self.list_automation_queue_snapshots(&transaction, now)?;
        let coordination_leases =
            self.list_automation_coordination_leases(&transaction, now)?;
        let repositories = self.list_automation_repositories(&transaction)?;

        let mut repository_statuses = Vec::with_capacity(repositories.len());
        for repository in repositories {
            let enabled_build_target_count =
                self.count_enabled_build_targets(&transaction, repository.id)?;
            let release_queue =
                self.list_repository_automation_releases(&transaction, repository.id)?;

            let queued_build_runs = release_queue
                .iter()
                .map(|release| release.queued_build_runs)
                .sum();
            let running_build_runs = release_queue
                .iter()
                .map(|release| release.running_build_runs)
                .sum();
            let queued_publish_runs = release_queue
                .iter()
                .map(|release| release.queued_publish_runs)
                .sum();
            let running_publish_runs = release_queue
                .iter()
                .map(|release| release.running_publish_runs)
                .sum();

            repository_statuses.push(RepositoryAutomationStatus {
                repository_id: repository.id,
                repository_name: repository.name,
                enabled: repository.enabled,
                polling_interval_seconds: repository.polling_interval_seconds,
                last_seen_tag: repository.last_seen_tag,
                enabled_build_target_count,
                pending_release_count: release_queue.len() as i64,
                queued_build_runs,
                running_build_runs,
                queued_publish_runs,
                running_publish_runs,
                release_queue,
            });
        }

        transaction.commit().map_err(sqlite_error)?;

        Ok(AutomationSnapshot {
            generated_at,
            queue_messages,
            coordination_leases,
            repositories: repository_statuses,
        })
    }

    fn resolve_release_engine_version(
        &self,
        release_run_id: i64,
        release: &ReleaseBuildPlanningState,
    ) -> io::Result<String> {
        if let Some(engine_version) = release
            .engine_version
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Ok(engine_version.to_owned());
        }

        let default_source_kind = match release.repository_source_mode.as_str() {
            SOURCE_MODE_LOCAL_WORKSPACE => RELEASE_SOURCE_KIND_LOCAL_WORKSPACE,
            _ => RELEASE_SOURCE_KIND_MANAGED_TAG,
        };
        let source = decode_release_source_metadata(
            &release.source_metadata_json,
            default_source_kind,
            Some(&release.git_tag),
            release.repository_local_path.as_deref(),
        )?;
        let git_auth = self.resolve_release_git_auth(release.credentials_id)?;
        let detected_engine_version = detect_release_engine_version_from_source(
            release.engine_kind,
            &release.repository_url,
            &source,
            &git_auth,
        )?;
        let connection = open_connection(&self.database_path)?;
        connection
            .execute(
                "
                UPDATE release_runs
                                SET engine_version = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                  AND (
                                        engine_version IS NULL
                                        OR TRIM(engine_version) = ''
                  )
                ",
                                params![detected_engine_version, release_run_id],
            )
            .map_err(sqlite_error)?;

                Ok(detected_engine_version)
    }

    /// Resolves one stored credential binding into Git authentication headers,
    /// including host-keyring-backed secret references.
    pub fn resolve_release_git_auth(
        &self,
        credentials_id: Option<i64>,
    ) -> io::Result<GitAuthOptions> {
        let Some(credentials_id) = credentials_id else {
            return Ok(GitAuthOptions::default());
        };

        let credentials = self.get_credential_record(credentials_id)?;
        let resolved_config_json =
            resolve_credential_secret_config_json(&credentials.kind, &credentials.config_json)?;
        git_auth_options_from_credentials(&credentials.kind, &resolved_config_json)
    }

    fn create_manual_release_dispatch(
        &self,
        input: &NormalizedManualReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        if !self.repository_exists(input.repository_id)? {
            return Err(not_found_error(format!(
                "release repository {} was not found",
                input.repository_id
            )));
        }
        self.reject_if_repository_build_work_active(input.repository_id)?;

        let metadata_json = manual_dispatch_metadata_json(&input.requested_via, &input.git_tag)?;
        let connection = open_connection(&self.database_path)?;
        let inserted = connection.execute(
            "
            INSERT INTO release_runs (
                repository_id,
                git_tag,
                git_commit,
                trigger_source,
                source_metadata_json,
                source_identity,
                status
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                input.repository_id,
                input.git_tag,
                nullable_string(&input.git_commit),
                TRIGGER_SOURCE_MANUAL,
                metadata_json,
                input.source_identity,
                ReleaseStatus::Detected.as_str(),
            ],
        );
        if let Err(error) = inserted {
            return Err(map_release_store_sqlite_error(error));
        }

        self.load_release_run_record(connection.last_insert_rowid())?.ok_or_else(|| {
            not_found_error("inserted manual release could not be reloaded")
        })
    }

    fn create_repository_poll_dispatch(
        &self,
        input: &NormalizedRepositoryPollDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        if !self.repository_exists(input.repository_id)? {
            return Err(not_found_error(format!(
                "release repository {} was not found",
                input.repository_id
            )));
        }
        self.reject_if_repository_build_work_active(input.repository_id)?;

        let metadata_json =
            repository_poll_metadata_json(&input.observed_via, &input.git_tag)?;
        let connection = open_connection(&self.database_path)?;
        let inserted = connection.execute(
            "
            INSERT INTO release_runs (
                repository_id,
                git_tag,
                git_commit,
                trigger_source,
                source_metadata_json,
                source_identity,
                status
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                input.repository_id,
                input.git_tag,
                nullable_string(&input.git_commit),
                TRIGGER_SOURCE_REPOSITORY_POLL,
                metadata_json,
                input.source_identity,
                ReleaseStatus::Detected.as_str(),
            ],
        );
        if let Err(error) = inserted {
            return Err(map_release_store_sqlite_error(error));
        }

        self.load_release_run_record(connection.last_insert_rowid())?.ok_or_else(|| {
            not_found_error("inserted repository-poll release could not be reloaded")
        })
    }

    fn rebuild_manual_release_dispatch(
        &self,
        input: &NormalizedManualReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        if !self.repository_exists(input.repository_id)? {
            return Err(not_found_error(format!(
                "release repository {} was not found",
                input.repository_id
            )));
        }
        self.reject_if_repository_build_work_active(input.repository_id)?;

        let Some(existing_release_run_id) = self.release_run_id_by_repository_tag_and_source_identity(
            input.repository_id,
            &input.git_tag,
            &input.source_identity,
        )? else {
            return self.create_manual_release_dispatch(input);
        };

        let metadata_json = manual_dispatch_metadata_json(&input.requested_via, &input.git_tag)?;
        let mut connection = open_connection(&self.database_path)?;
        let cleanup_paths = Self::collect_release_run_rebuild_cleanup_paths(
            &connection,
            existing_release_run_id,
        )?;
        Self::remove_release_run_rebuild_cleanup_paths(
            &cleanup_paths,
            existing_release_run_id,
        )?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "DELETE FROM build_runs WHERE release_run_id = ?",
                [existing_release_run_id],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    trigger_source = ?,
                    trigger_rule_id = NULL,
                    source_metadata_json = ?,
                    source_identity = ?,
                    status = ?,
                    started_at = NULL,
                    finished_at = NULL,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    nullable_string(&input.git_commit),
                    TRIGGER_SOURCE_MANUAL,
                    metadata_json,
                    input.source_identity,
                    ReleaseStatus::Detected.as_str(),
                    existing_release_run_id,
                ],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;

        self.load_release_run_record(existing_release_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "rebuilt release run {existing_release_run_id} could not be reloaded"
            ))
        })
    }

    fn create_on_demand_release_dispatch(
        &self,
        input: &NormalizedOnDemandReleaseDispatchInput,
    ) -> io::Result<ReleaseRunRecord> {
        if !self.repository_exists(input.repository_id)? {
            return Err(not_found_error(format!(
                "release repository {} was not found",
                input.repository_id
            )));
        }
        self.reject_if_repository_build_work_active(input.repository_id)?;

        let metadata_json = on_demand_dispatch_metadata_json(input)?;
        let connection = open_connection(&self.database_path)?;
        let inserted = connection.execute(
            "
            INSERT INTO release_runs (
                repository_id,
                git_tag,
                git_commit,
                trigger_source,
                source_metadata_json,
                source_identity,
                status
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                input.repository_id,
                input.git_tag,
                nullable_string(&input.git_commit),
                TRIGGER_SOURCE_MANUAL,
                metadata_json,
                input.source_identity,
                ReleaseStatus::Detected.as_str(),
            ],
        );
        if let Err(error) = inserted {
            return Err(map_release_store_sqlite_error(error));
        }

        self.load_release_run_record(connection.last_insert_rowid())?.ok_or_else(|| {
            not_found_error("inserted on-demand release could not be reloaded")
        })
    }

    fn rebuild_release_dispatch_by_id(
        &self,
        release_run_id: i64,
        requested_via: &str,
    ) -> io::Result<ReleaseRunRecord> {
        let existing = self
            .load_release_run_record(release_run_id)?
            .ok_or_else(|| not_found_error(format!("release run {release_run_id} was not found")))?;
        self.reject_if_repository_build_work_active(existing.repository_id)?;

        let metadata_json = rebuild_release_metadata_json(&existing, requested_via)?;
        let mut connection = open_connection(&self.database_path)?;
        let cleanup_paths =
            Self::collect_release_run_rebuild_cleanup_paths(&connection, release_run_id)?;
        Self::remove_release_run_rebuild_cleanup_paths(&cleanup_paths, release_run_id)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        transaction
            .execute(
                "DELETE FROM build_runs WHERE release_run_id = ?",
                [release_run_id],
            )
            .map_err(sqlite_error)?;
        transaction
            .execute(
                "
                UPDATE release_runs
                SET trigger_source = ?,
                    trigger_rule_id = NULL,
                    source_metadata_json = ?,
                    status = ?,
                    started_at = NULL,
                    finished_at = NULL,
                    error_message = NULL,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    TRIGGER_SOURCE_MANUAL,
                    metadata_json,
                    ReleaseStatus::Detected.as_str(),
                    release_run_id,
                ],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;

        self.load_release_run_record(release_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "rebuilt release run {release_run_id} could not be reloaded"
            ))
        })
    }

fn collect_release_run_rebuild_cleanup_paths(
    connection: &Connection,
    release_run_id: i64,
) -> io::Result<Vec<PathBuf>> {
    let mut statement = connection
        .prepare(
            "
            SELECT workspace_path, artifact_root_path
            FROM build_runs
            WHERE release_run_id = ?
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([release_run_id], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .map_err(sqlite_error)?;

    let mut paths = HashSet::new();
    for row in rows {
        let (workspace_path, artifact_root_path) = row.map_err(sqlite_error)?;
        for candidate in [workspace_path, artifact_root_path] {
            let Some(path) = candidate
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };

            paths.insert(PathBuf::from(path));
        }
    }

    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn remove_release_run_rebuild_cleanup_paths(
    paths: &[PathBuf],
    release_run_id: i64,
) -> io::Result<()> {
    for path in paths {
        if !path.exists() {
            continue;
        }

        let metadata = fs::metadata(path).map_err(|error| {
            io::Error::other(format!(
                "inspect rebuild cleanup path '{}' for release run {release_run_id}: {error}",
                path.display()
            ))
        })?;
        if metadata.is_dir() {
            fs::remove_dir_all(path).map_err(|error| {
                io::Error::other(format!(
                    "remove rebuild cleanup directory '{}' for release run {release_run_id}: {error}",
                    path.display()
                ))
            })?;
        } else {
            fs::remove_file(path).map_err(|error| {
                io::Error::other(format!(
                    "remove rebuild cleanup file '{}' for release run {release_run_id}: {error}",
                    path.display()
                ))
            })?;
        }
    }

    Ok(())
}

    fn queue_release_run_locked(&self, release_run_id: i64) -> io::Result<ReleaseRunRecord> {
        let record = self
            .load_release_run_record(release_run_id)?
            .ok_or_else(|| not_found_error(format!("release run {release_run_id} was not found")))?;
        if record.status == ReleaseStatus::Queued.as_str() {
            return Ok(record);
        }

        let claimed_key = release_dispatch_idempotency_key(record.id);
        if !self.claim_idempotency(&claimed_key, DISPATCH_IDEMPOTENCY_TTL)? {
            return Err(io::Error::new(
                ErrorKind::AlreadyExists,
                format!("release run {release_run_id} dispatch was already claimed"),
            ));
        }

        let payload = match serde_json::to_vec(&ReleaseDispatchJob::from(&record)) {
            Ok(payload) => payload,
            Err(error) => {
                let _ = self.forget_idempotency(&claimed_key);
                return Err(io::Error::new(ErrorKind::InvalidData, error));
            }
        };
        if let Err(error) = self.enqueue(RELEASE_RUN_QUEUE_NAME, &payload) {
            let _ = self.forget_idempotency(&claimed_key);
            return Err(error);
        }

        self.mark_release_run_queued(record.id)
    }

    fn dispatch_build_run_locked(&self, build_run_id: i64) -> io::Result<QueueDispatchOutcome> {
        let state = self
            .load_build_run_dispatch_state(build_run_id)?
            .ok_or_else(|| not_found_error(format!("build run {build_run_id} was not found")))?;
        if state.status != BuildStatus::Queued.as_str() {
            return Err(invalid_input_error(format!(
                "build run {build_run_id} must be queued before dispatch"
            )));
        }

        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        if let Some(running_build_run_id) =
            self.running_build_run_id_in_transaction(&transaction, state.job.release_run_id)?
        {
            transaction.commit().map_err(sqlite_error)?;
            return Err(io::Error::new(
                ErrorKind::WouldBlock,
                format!(
                    "release run {} already has running build run {}",
                    state.job.release_run_id,
                    running_build_run_id
                ),
            ));
        }
        let next_queued_build_run_id =
            self.next_queued_build_run_id_in_transaction(&transaction, state.job.release_run_id)?;
        transaction.commit().map_err(sqlite_error)?;
        if next_queued_build_run_id != Some(build_run_id) {
            return Err(io::Error::new(
                ErrorKind::WouldBlock,
                format!(
                    "build run {build_run_id} is not the next queued build for release run {}",
                    state.job.release_run_id
                ),
            ));
        }

        let claimed_key = build_dispatch_idempotency_key(build_run_id, &state.created_at);
        if !self.claim_idempotency(&claimed_key, DISPATCH_IDEMPOTENCY_TTL)? {
            return Ok(QueueDispatchOutcome::AlreadyClaimed);
        }

        let payload = match serde_json::to_vec(&state.job) {
            Ok(payload) => payload,
            Err(error) => {
                let _ = self.forget_idempotency(&claimed_key);
                return Err(io::Error::new(ErrorKind::InvalidData, error));
            }
        };

        if let Err(error) = self.enqueue(BUILD_RUN_QUEUE_NAME, &payload) {
            let _ = self.forget_idempotency(&claimed_key);
            return Err(error);
        }

        Ok(QueueDispatchOutcome::Enqueued)
    }

    fn dispatch_publish_run_locked(
        &self,
        publish_run_id: i64,
    ) -> io::Result<QueueDispatchOutcome> {
        let state = self.load_publish_run_dispatch_state(publish_run_id)?.ok_or_else(|| {
            not_found_error(format!("publish run {publish_run_id} was not found"))
        })?;
        if state.status != PublishStatus::Queued.as_str() {
            return Err(invalid_input_error(format!(
                "publish run {publish_run_id} must be queued before dispatch"
            )));
        }

        let claimed_key = publish_dispatch_idempotency_key(publish_run_id, &state.created_at);
        if !self.claim_idempotency(&claimed_key, DISPATCH_IDEMPOTENCY_TTL)? {
            return Ok(QueueDispatchOutcome::AlreadyClaimed);
        }

        let payload = match serde_json::to_vec(&state.job) {
            Ok(payload) => payload,
            Err(error) => {
                let _ = self.forget_idempotency(&claimed_key);
                return Err(io::Error::new(ErrorKind::InvalidData, error));
            }
        };

        if let Err(error) = self.enqueue(PUBLISH_RUN_QUEUE_NAME, &payload) {
            let _ = self.forget_idempotency(&claimed_key);
            return Err(error);
        }

        Ok(QueueDispatchOutcome::Enqueued)
    }

    fn claim_next_once(
        &self,
        queue_name: &str,
        worker_name: &str,
        lease_ttl_millis: i64,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let now = unix_timestamp_millis()?;
        let lease_expires_at_unix_millis = now + lease_ttl_millis;
        let lease_token = next_token("queue")?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let selected = transaction
            .query_row(
                "
                SELECT id, payload, dequeue_count
                FROM worker_queue_messages
                WHERE queue_name = ?
                  AND (
                    lease_expires_at_unix_millis IS NULL
                    OR lease_expires_at_unix_millis <= ?
                  )
                ORDER BY id
                LIMIT 1
                ",
                params![queue_name, now],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(sqlite_error)?;

        let Some((message_id, payload, dequeue_count)) = selected else {
            transaction.commit().map_err(sqlite_error)?;
            return Ok(None);
        };

        let updated = transaction
            .execute(
                "
                UPDATE worker_queue_messages
                SET leased_by = ?,
                    lease_token = ?,
                    lease_expires_at_unix_millis = ?,
                    dequeue_count = dequeue_count + 1
                WHERE id = ?
                  AND queue_name = ?
                  AND (
                    lease_expires_at_unix_millis IS NULL
                    OR lease_expires_at_unix_millis <= ?
                  )
                ",
                params![
                    worker_name,
                    lease_token,
                    lease_expires_at_unix_millis,
                    message_id,
                    queue_name,
                    now,
                ],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)?;

        if updated == 0 {
            return Ok(None);
        }

        Ok(Some(ClaimedQueueMessage {
            id: message_id,
            queue_name: queue_name.to_owned(),
            payload,
            leased_by: worker_name.to_owned(),
            lease_token,
            lease_expires_at_unix_millis,
            dequeue_count: dequeue_count + 1,
        }))
    }

    fn claim_next_build_job_once(
        &self,
        worker_name: &str,
        lease_ttl_millis: i64,
        concurrency: &RuntimeConcurrencySettings,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let now = unix_timestamp_millis()?;
        let lease_expires_at_unix_millis = now + lease_ttl_millis;
        let lease_token = next_token("queue")?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let active_build_runs_on_host = self.active_build_run_count(&transaction)?;
        if active_build_runs_on_host >= concurrency.max_concurrent_build_runs {
            transaction.commit().map_err(sqlite_error)?;
            return Ok(None);
        }

        let messages = self.available_queue_messages(&transaction, BUILD_RUN_QUEUE_NAME, now)?;
        for message in messages {
            let job = match serde_json::from_slice::<BuildQueueJob>(&message.payload) {
                Ok(job) if job.build_run_id > 0 => job,
                _ => {
                    self.delete_queue_message(&transaction, message.id)?;
                    continue;
                }
            };

            let Some(run) = self.load_build_run_claim_state(&transaction, job.build_run_id)? else {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            };
            if run.status != BuildStatus::Queued.as_str() {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            }
            if !self.repository_release_lane_available(
                &transaction,
                run.repository_id,
                run.release_run_id,
                concurrency.max_active_releases_per_repository,
            )? {
                continue;
            }
            if self
                .running_build_run_id_in_transaction(&transaction, run.release_run_id)?
                .is_some()
            {
                continue;
            }
            if self.next_queued_build_run_id_in_transaction(&transaction, run.release_run_id)?
                != Some(job.build_run_id)
            {
                continue;
            }

            let claimed = self.claim_specific_message(
                &transaction,
                message,
                BUILD_RUN_QUEUE_NAME,
                worker_name,
                &lease_token,
                lease_expires_at_unix_millis,
                now,
            )?;
            transaction.commit().map_err(sqlite_error)?;
            return Ok(claimed);
        }

        transaction.commit().map_err(sqlite_error)?;
        Ok(None)
    }

    fn claim_next_release_job_once(
        &self,
        worker_name: &str,
        lease_ttl_millis: i64,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let now = unix_timestamp_millis()?;
        let lease_expires_at_unix_millis = now + lease_ttl_millis;
        let lease_token = next_token("queue")?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let messages = self.available_queue_messages(&transaction, RELEASE_RUN_QUEUE_NAME, now)?;
        for message in messages {
            let job = match decode_release_dispatch_job(&message.payload) {
                Ok(job) => job,
                Err(_) => {
                    self.delete_queue_message(&transaction, message.id)?;
                    continue;
                }
            };

            let Some(release) = self.load_release_build_planning_state(&transaction, job.release_run_id)?
            else {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            };
            if release.repository_id != job.repository_id {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            }
            if release.status != ReleaseStatus::Detected.as_str()
                && release.status != ReleaseStatus::Queued.as_str()
            {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            }

            let claimed = self.claim_specific_message(
                &transaction,
                message,
                RELEASE_RUN_QUEUE_NAME,
                worker_name,
                &lease_token,
                lease_expires_at_unix_millis,
                now,
            )?;
            transaction.commit().map_err(sqlite_error)?;
            return Ok(claimed);
        }

        transaction.commit().map_err(sqlite_error)?;
        Ok(None)
    }

    fn claim_next_publish_job_once(
        &self,
        worker_name: &str,
        lease_ttl_millis: i64,
        concurrency: &RuntimeConcurrencySettings,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let now = unix_timestamp_millis()?;
        let lease_expires_at_unix_millis = now + lease_ttl_millis;
        let lease_token = next_token("queue")?;
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;

        let active_publish_runs_on_host = self.active_publish_run_count(&transaction)?;
        if active_publish_runs_on_host >= concurrency.max_concurrent_publish_runs {
            transaction.commit().map_err(sqlite_error)?;
            return Ok(None);
        }

        let messages = self.available_queue_messages(&transaction, PUBLISH_RUN_QUEUE_NAME, now)?;
        for message in messages {
            let job = match serde_json::from_slice::<PublishQueueJob>(&message.payload) {
                Ok(job) if job.publish_run_id > 0 => job,
                _ => {
                    self.delete_queue_message(&transaction, message.id)?;
                    continue;
                }
            };

            let Some(run) = self.load_publish_run_claim_state(&transaction, job.publish_run_id)? else {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            };
            if run.status != PublishStatus::Queued.as_str() {
                self.delete_queue_message(&transaction, message.id)?;
                continue;
            }
            if let Some(artifact_id) = run.artifact_id {
                if has_incomplete_prior_publish_run_for_artifact(
                    &transaction,
                    job.publish_run_id,
                    artifact_id,
                )? {
                    continue;
                }
            }

            let claimed = self.claim_specific_message(
                &transaction,
                message,
                PUBLISH_RUN_QUEUE_NAME,
                worker_name,
                &lease_token,
                lease_expires_at_unix_millis,
                now,
            )?;
            transaction.commit().map_err(sqlite_error)?;
            return Ok(claimed);
        }

        transaction.commit().map_err(sqlite_error)?;
        Ok(None)
    }

    fn available_queue_messages(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        queue_name: &str,
        now: i64,
    ) -> io::Result<Vec<AvailableQueueMessage>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id, payload, dequeue_count
                FROM worker_queue_messages
                WHERE queue_name = ?
                  AND (
                    lease_expires_at_unix_millis IS NULL
                    OR lease_expires_at_unix_millis <= ?
                  )
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(params![queue_name, now], |row| {
                Ok(AvailableQueueMessage {
                    id: row.get(0)?,
                    payload: row.get(1)?,
                    dequeue_count: row.get(2)?,
                })
            })
            .map_err(sqlite_error)?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(row.map_err(sqlite_error)?);
        }

        Ok(messages)
    }

    fn claim_specific_message(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        message: AvailableQueueMessage,
        queue_name: &str,
        worker_name: &str,
        lease_token: &str,
        lease_expires_at_unix_millis: i64,
        now: i64,
    ) -> io::Result<Option<ClaimedQueueMessage>> {
        let updated = transaction
            .execute(
                "
                UPDATE worker_queue_messages
                SET leased_by = ?,
                    lease_token = ?,
                    lease_expires_at_unix_millis = ?,
                    dequeue_count = dequeue_count + 1
                WHERE id = ?
                  AND queue_name = ?
                  AND (
                    lease_expires_at_unix_millis IS NULL
                    OR lease_expires_at_unix_millis <= ?
                  )
                ",
                params![
                    worker_name,
                    lease_token,
                    lease_expires_at_unix_millis,
                    message.id,
                    queue_name,
                    now,
                ],
            )
            .map_err(sqlite_error)?;

        if updated == 0 {
            return Ok(None);
        }

        Ok(Some(ClaimedQueueMessage {
            id: message.id,
            queue_name: queue_name.to_owned(),
            payload: message.payload,
            leased_by: worker_name.to_owned(),
            lease_token: lease_token.to_owned(),
            lease_expires_at_unix_millis,
            dequeue_count: message.dequeue_count + 1,
        }))
    }

    fn delete_queue_message(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        message_id: i64,
    ) -> io::Result<()> {
        transaction
            .execute(
                "DELETE FROM worker_queue_messages WHERE id = ?",
                [message_id],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    fn active_build_run_count(
        &self,
        transaction: &rusqlite::Transaction<'_>,
    ) -> io::Result<u32> {
        transaction
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE status = ?",
                [BuildStatus::Running.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)
    }

    fn active_publish_run_count(
        &self,
        transaction: &rusqlite::Transaction<'_>,
    ) -> io::Result<u32> {
        transaction
            .query_row(
                "SELECT COUNT(1) FROM publish_runs WHERE status = ?",
                [PublishStatus::Running.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)
    }

    fn load_build_run_claim_state(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        build_run_id: i64,
    ) -> io::Result<Option<BuildRunClaimState>> {
        transaction
            .query_row(
                "
                SELECT br.release_run_id, rr.repository_id, br.status
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE br.id = ?
                ",
                [build_run_id],
                |row| {
                    Ok(BuildRunClaimState {
                        release_run_id: row.get(0)?,
                        repository_id: row.get(1)?,
                        status: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_publish_run_claim_state(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        publish_run_id: i64,
    ) -> io::Result<Option<PublishRunClaimState>> {
        transaction
            .query_row(
                "SELECT artifact_id, status FROM publish_runs WHERE id = ?",
                [publish_run_id],
                |row| {
                    Ok(PublishRunClaimState {
                        artifact_id: row.get(0)?,
                        status: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_release_build_planning_state(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Option<ReleaseBuildPlanningState>> {
        transaction
            .query_row(
                "
                SELECT rr.repository_id,
                      r.source_mode,
                       r.repo_url,
                      r.local_path,
                     r.engine_kind,
                       r.credentials_id,
                       rr.git_tag,
                      rr.source_metadata_json,
                      rr.engine_version,
                       rr.status
                FROM release_runs rr
                JOIN repositories r ON r.id = rr.repository_id
                  WHERE rr.id = ?
                ",
                [release_run_id],
                scan_release_build_planning_state,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_release_build_planning_state_from_connection(
        &self,
        connection: &Connection,
        release_run_id: i64,
    ) -> io::Result<Option<ReleaseBuildPlanningState>> {
        connection
            .query_row(
                "
                SELECT rr.repository_id,
                      r.source_mode,
                       r.repo_url,
                      r.local_path,
                     r.engine_kind,
                       r.credentials_id,
                       rr.git_tag,
                      rr.source_metadata_json,
                      rr.engine_version,
                       rr.status
                FROM release_runs rr
                JOIN repositories r ON r.id = rr.repository_id
                WHERE rr.id = ?
                ",
                [release_run_id],
                scan_release_build_planning_state,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_release_run_record(&self, release_run_id: i64) -> io::Result<Option<ReleaseRunRecord>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
                       repository_id,
                       git_tag,
                       git_commit,
                       trigger_source,
                       trigger_rule_id,
                       source_metadata_json,
                      source_identity,
                      engine_version,
                       status,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM release_runs
                WHERE id = ?
                ",
                [release_run_id],
                scan_release_run_record,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_release_run_record_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Option<ReleaseRunRecord>> {
        transaction
            .query_row(
                "
                SELECT id,
                       repository_id,
                       git_tag,
                       git_commit,
                       trigger_source,
                       trigger_rule_id,
                       source_metadata_json,
                      source_identity,
                      engine_version,
                       status,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM release_runs
                WHERE id = ?
                ",
                [release_run_id],
                scan_release_run_record,
            )
            .optional()
            .map_err(sqlite_error)
    }

    /// Recomputes one release status from the durable child build and publish rows.
    fn reconcile_release_run_status(&self, release_run_id: i64) -> io::Result<()> {
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        self.reconcile_release_run_status_in_transaction(&transaction, release_run_id)?;
        transaction.commit().map_err(sqlite_error)
    }

    /// Repairs non-terminal release rows that no longer match their child execution state.
    fn reconcile_release_run_statuses(&self) -> io::Result<()> {
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sqlite_error)?;
        let release_run_ids = self.list_open_release_run_ids(&transaction)?;

        for release_run_id in release_run_ids {
            self.reconcile_release_run_status_in_transaction(&transaction, release_run_id)?;
        }

        transaction.commit().map_err(sqlite_error)
    }

    fn load_build_run_record(&self, build_run_id: i64) -> io::Result<Option<BuildRunRecord>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
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
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM build_runs
                WHERE id = ?
                ",
                [build_run_id],
                scan_build_run_record,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_build_run_stage_record(
        &self,
        build_run_id: i64,
        step_key: &str,
    ) -> io::Result<Option<BuildRunStageRecord>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
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
                       created_at,
                       updated_at
                FROM build_run_steps
                WHERE build_run_id = ?
                  AND step_key = ?
                ",
                params![build_run_id, step_key],
                scan_build_run_stage_record,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_publish_run_record(
        &self,
        publish_run_id: i64,
    ) -> io::Result<Option<PublishRunRecord>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT id,
                       release_run_id,
                       build_run_id,
                       publish_target_id,
                       artifact_id,
                       status,
                       destination_ref,
                      execution_contract_json,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM publish_runs
                WHERE id = ?
                ",
                [publish_run_id],
                scan_publish_run_record,
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn build_run_transition_error(
        &self,
        build_run_id: i64,
        expected_status: &str,
        action: &str,
    ) -> io::Error {
        match self.load_build_run_record(build_run_id) {
            Ok(Some(run)) => invalid_input_error(format!(
                "build run {build_run_id} must be {expected_status} before {action}, got {:?}",
                run.status
            )),
            Ok(None) => not_found_error(format!("build run {build_run_id} was not found")),
            Err(error) => error,
        }
    }

    fn publish_run_transition_error(
        &self,
        publish_run_id: i64,
        expected_status: &str,
        action: &str,
    ) -> io::Error {
        match self.load_publish_run_record(publish_run_id) {
            Ok(Some(run)) => invalid_input_error(format!(
                "publish run {publish_run_id} must be {expected_status} before {action}, got {:?}",
                run.status
            )),
            Ok(None) => not_found_error(format!("publish run {publish_run_id} was not found")),
            Err(error) => error,
        }
    }

    fn process_claimed_release_job(
        &self,
        job: &ReleaseDispatchJob,
    ) -> io::Result<ReleaseJobDisposition> {
        let Some(release) = self.wait_for_release_queue_ready(job.release_run_id)? else {
            return Ok(ReleaseJobDisposition::Acknowledge);
        };
        if release.repository_id != job.repository_id {
            return Ok(ReleaseJobDisposition::Acknowledge);
        }
        if release.status == ReleaseStatus::Detected.as_str() {
            return Ok(ReleaseJobDisposition::RetryLater);
        }
        if release.status != ReleaseStatus::Queued.as_str() {
            return Ok(ReleaseJobDisposition::Acknowledge);
        }

        let busy = self.advance_repository_release_queue(release.repository_id)?;
        if busy && self.build_run_count_for_release(release.id)? == 0 {
            return Ok(ReleaseJobDisposition::RetryLater);
        }

        Ok(ReleaseJobDisposition::Acknowledge)
    }

    fn wait_for_release_queue_ready(
        &self,
        release_run_id: i64,
    ) -> io::Result<Option<ReleaseRunRecord>> {
        for _ in 0..20 {
            let Some(record) = self.load_release_run_record(release_run_id)? else {
                return Ok(None);
            };
            if record.status != ReleaseStatus::Detected.as_str() {
                return Ok(Some(record));
            }

            thread::sleep(Duration::from_millis(50));
        }

        self.load_release_run_record(release_run_id)
    }

    /// Advances queued releases for one repository until active build work blocks further progress.
    pub fn advance_repository_release_queue(&self, repository_id: i64) -> io::Result<bool> {
        require_positive_identifier(repository_id, "repository id")?;

        let queued_releases = self.list_queued_releases_by_repository(repository_id)?;
        for release in queued_releases {
            let runs = self.list_build_runs_by_release_readonly(release.id)?;
            if build_runs_block_repository_queue(&runs) {
                return Ok(true);
            }
            if !runs.is_empty() {
                continue;
            }

            let lock_name = release_planning_lock_key(release.id);
            let Some(lock) = self.acquire_lock(&lock_name, RELEASE_PLANNING_LOCK_TTL)? else {
                return Ok(true);
            };

            let planned = self.plan_release_builds(release.id);
            let _ = self.release_lock(&lock.name, &lock.token);

            match planned {
                Ok(_) => {}
                Err(error) if is_release_planning_noop_error(&error) => continue,
                Err(error) => return Err(error),
            }

            let planned_runs = self.list_build_runs_by_release_readonly(release.id)?;
            if build_runs_block_repository_queue(&planned_runs) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn mark_release_run_queued(&self, release_run_id: i64) -> io::Result<ReleaseRunRecord> {
        let connection = open_connection(&self.database_path)?;
        let updated = connection
            .execute(
                "
                UPDATE release_runs
                SET status = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![ReleaseStatus::Queued.as_str(), release_run_id],
            )
            .map_err(sqlite_error)?;
        if updated == 0 {
            return Err(not_found_error(format!(
                "release run {release_run_id} was not found"
            )));
        }

        self.load_release_run_record(release_run_id)?.ok_or_else(|| {
            not_found_error(format!(
                "queued release run {release_run_id} could not be reloaded"
            ))
        })
    }

    fn load_repository_project_removal_record(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<RepositoryProjectRemovalRecord> {
        transaction
            .query_row(
                "
                SELECT id, name
                FROM repositories
                WHERE id = ?
                ",
                [repository_id],
                |row| {
                    Ok(RepositoryProjectRemovalRecord {
                        id: row.get(0)?,
                        name: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)?
            .ok_or_else(|| not_found_error(format!("repository {repository_id} was not found")))
    }

    fn load_repository_project_removal_runtime_state(
        transaction: &Transaction<'_>,
        repository_id: i64,
        collect_paths: bool,
    ) -> io::Result<RepositoryProjectRemovalRuntimeState> {
        let release_run_ids = Self::load_release_run_ids_for_repository(
            transaction,
            repository_id,
        )?;
        let build_run_ids = Self::load_build_run_ids_for_repository(
            transaction,
            repository_id,
        )?;
        let publish_run_ids = Self::load_publish_run_ids_for_repository(
            transaction,
            repository_id,
        )?;
        let queue_messages = Self::load_target_queue_messages(
            transaction,
            repository_id,
            &release_run_ids,
            &build_run_ids,
            &publish_run_ids,
        )?;
        let coordination_lease_names = Self::load_target_coordination_lease_names(
            transaction,
            &release_run_ids,
            &build_run_ids,
            &publish_run_ids,
        )?;
        let idempotency_keys = Self::load_target_idempotency_keys(
            transaction,
            &release_run_ids,
            &build_run_ids,
            &publish_run_ids,
        )?;
        let (directory_paths, file_paths) = if collect_paths {
            Self::load_repository_project_removal_paths(transaction, repository_id)?
        } else {
            (Vec::new(), Vec::new())
        };

        Ok(RepositoryProjectRemovalRuntimeState {
            release_run_ids,
            build_run_ids,
            publish_run_ids,
            queue_messages,
            coordination_lease_names,
            idempotency_keys,
            directory_paths,
            file_paths,
        })
    }

    fn reject_repository_project_removal_if_active(
        transaction: &Transaction<'_>,
        repository_id: i64,
        repository_name: &str,
        runtime_state: &RepositoryProjectRemovalRuntimeState,
    ) -> io::Result<()> {
        let has_running_process_work =
            Self::repository_has_running_process_work_in_transaction(
                transaction,
                repository_id,
            )?;
        let has_leased_queue_messages = runtime_state
            .queue_messages
            .iter()
            .any(|message| message.leased_by.is_some());

        if !(has_running_process_work || has_leased_queue_messages) {
            return Ok(());
        }

        Err(io::Error::new(
            ErrorKind::WouldBlock,
            format!(
                "repository project {repository_name:?} cannot be removed while related runtime work is active"
            ),
        ))
    }

    fn repository_has_running_process_work_in_transaction(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<bool> {
        let running_release_count: i64 = transaction
            .query_row(
                "
                SELECT COUNT(1)
                FROM release_runs
                WHERE repository_id = ?
                  AND status = ?
                ",
                params![repository_id, ReleaseStatus::Running.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        if running_release_count > 0 {
            return Ok(true);
        }

        let running_build_count: i64 = transaction
            .query_row(
                "
                SELECT COUNT(1)
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE rr.repository_id = ?
                  AND br.status = ?
                ",
                params![repository_id, BuildStatus::Running.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        if running_build_count > 0 {
            return Ok(true);
        }

        let running_publish_count: i64 = transaction
            .query_row(
                "
                SELECT COUNT(1)
                FROM publish_runs pr
                JOIN release_runs rr ON rr.id = pr.release_run_id
                WHERE rr.repository_id = ?
                  AND pr.status = ?
                ",
                params![repository_id, PublishStatus::Running.as_str()],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        Ok(running_publish_count > 0)
    }

    fn load_release_run_ids_for_repository(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<HashSet<i64>> {
        Self::load_identifier_set_for_repository(
            transaction,
            "
            SELECT id
            FROM release_runs
            WHERE repository_id = ?
            ",
            repository_id,
        )
    }

    fn load_build_run_ids_for_repository(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<HashSet<i64>> {
        Self::load_identifier_set_for_repository(
            transaction,
            "
            SELECT br.id
            FROM build_runs br
            JOIN release_runs rr ON rr.id = br.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
        )
    }

    fn load_publish_run_ids_for_repository(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<HashSet<i64>> {
        Self::load_identifier_set_for_repository(
            transaction,
            "
            SELECT pr.id
            FROM publish_runs pr
            JOIN release_runs rr ON rr.id = pr.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
        )
    }

    fn load_identifier_set_for_repository(
        transaction: &Transaction<'_>,
        sql: &str,
        repository_id: i64,
    ) -> io::Result<HashSet<i64>> {
        let mut statement = transaction.prepare(sql).map_err(sqlite_error)?;
        let rows = statement
            .query_map([repository_id], |row| row.get::<_, i64>(0))
            .map_err(sqlite_error)?;

        let mut identifiers = HashSet::new();
        for row in rows {
            identifiers.insert(row.map_err(sqlite_error)?);
        }

        Ok(identifiers)
    }

    fn load_target_queue_messages(
        transaction: &Transaction<'_>,
        repository_id: i64,
        release_run_ids: &HashSet<i64>,
        build_run_ids: &HashSet<i64>,
        publish_run_ids: &HashSet<i64>,
    ) -> io::Result<Vec<TargetQueueMessage>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id, queue_name, payload, leased_by
                FROM worker_queue_messages
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(sqlite_error)?;

        let mut queue_messages = Vec::new();
        for row in rows {
            let (message_id, queue_name, payload, leased_by) = row.map_err(sqlite_error)?;
            let belongs_to_repository = match queue_name.as_str() {
                RELEASE_RUN_QUEUE_NAME => serde_json::from_slice::<ReleaseDispatchJob>(&payload)
                    .map(|job| job.repository_id == repository_id)
                    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?,
                BUILD_RUN_QUEUE_NAME => serde_json::from_slice::<BuildDispatchJob>(&payload)
                    .map(|job| {
                        release_run_ids.contains(&job.release_run_id)
                            || build_run_ids.contains(&job.build_run_id)
                    })
                    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?,
                PUBLISH_RUN_QUEUE_NAME => serde_json::from_slice::<PublishDispatchJob>(&payload)
                    .map(|job| {
                        release_run_ids.contains(&job.release_run_id)
                            || build_run_ids.contains(&job.build_run_id)
                            || publish_run_ids.contains(&job.publish_run_id)
                    })
                    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?,
                _ => false,
            };

            if belongs_to_repository {
                queue_messages.push(TargetQueueMessage {
                    id: message_id,
                    leased_by: leased_by
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty()),
                });
            }
        }

        Ok(queue_messages)
    }

    fn load_target_coordination_lease_names(
        transaction: &Transaction<'_>,
        release_run_ids: &HashSet<i64>,
        build_run_ids: &HashSet<i64>,
        publish_run_ids: &HashSet<i64>,
    ) -> io::Result<Vec<String>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT name
                FROM worker_coordination_leases
                ORDER BY name ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_error)?;

        let mut names = Vec::new();
        for row in rows {
            let name = row.map_err(sqlite_error)?;
            if Self::matches_release_lock_name(&name, release_run_ids)
                || Self::matches_build_lock_name(&name, build_run_ids)
                || Self::matches_publish_lock_name(&name, publish_run_ids)
            {
                names.push(name);
            }
        }

        Ok(names)
    }

    fn load_target_idempotency_keys(
        transaction: &Transaction<'_>,
        release_run_ids: &HashSet<i64>,
        build_run_ids: &HashSet<i64>,
        publish_run_ids: &HashSet<i64>,
    ) -> io::Result<Vec<String>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT idempotency_key
                FROM worker_idempotency_keys
                ORDER BY idempotency_key ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sqlite_error)?;

        let mut keys = Vec::new();
        for row in rows {
            let key = row.map_err(sqlite_error)?;
            if Self::matches_release_idempotency_key(&key, release_run_ids)
                || Self::matches_build_idempotency_key(&key, build_run_ids)
                || Self::matches_publish_idempotency_key(&key, publish_run_ids)
            {
                keys.push(key);
            }
        }

        Ok(keys)
    }

    fn load_repository_project_removal_paths(
        transaction: &Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<(Vec<String>, Vec<String>)> {
        let mut directory_paths = HashSet::new();
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT workspace_path
            FROM build_runs br
            JOIN release_runs rr ON rr.id = br.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
            &mut directory_paths,
        )?;
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT artifact_root_path
            FROM build_runs br
            JOIN release_runs rr ON rr.id = br.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
            &mut directory_paths,
        )?;
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT ecr.workspace_path
            FROM execution_cleanup_records ecr
            LEFT JOIN build_runs br ON br.id = ecr.build_run_id
            LEFT JOIN release_runs brr ON brr.id = br.release_run_id
            LEFT JOIN publish_runs pr ON pr.id = ecr.publish_run_id
            LEFT JOIN release_runs prr ON prr.id = pr.release_run_id
            WHERE COALESCE(brr.repository_id, prr.repository_id) = ?
            ",
            repository_id,
            &mut directory_paths,
        )?;

        let mut file_paths = HashSet::new();
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT log_path
            FROM build_runs br
            JOIN release_runs rr ON rr.id = br.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
            &mut file_paths,
        )?;
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT brs.log_path
            FROM build_run_steps brs
            JOIN build_runs br ON br.id = brs.build_run_id
            JOIN release_runs rr ON rr.id = br.release_run_id
            WHERE rr.repository_id = ?
            ",
            repository_id,
            &mut file_paths,
        )?;
        Self::collect_string_values_for_repository_query(
            transaction,
            "
            SELECT ref.path
            FROM retained_execution_files ref
            LEFT JOIN build_runs br ON br.id = ref.build_run_id
            LEFT JOIN release_runs brr ON brr.id = br.release_run_id
            LEFT JOIN publish_runs pr ON pr.id = ref.publish_run_id
            LEFT JOIN release_runs prr ON prr.id = pr.release_run_id
            WHERE COALESCE(brr.repository_id, prr.repository_id) = ?
            ",
            repository_id,
            &mut file_paths,
        )?;
        let mut directory_paths = directory_paths.into_iter().collect::<Vec<_>>();
        directory_paths.sort();
        let mut file_paths = file_paths.into_iter().collect::<Vec<_>>();
        file_paths.sort();

        Ok((directory_paths, file_paths))
    }

    fn collect_string_values_for_repository_query(
        transaction: &Transaction<'_>,
        sql: &str,
        repository_id: i64,
        values: &mut HashSet<String>,
    ) -> io::Result<()> {
        let mut statement = transaction.prepare(sql).map_err(sqlite_error)?;
        let rows = statement
            .query_map([repository_id], |row| row.get::<_, Option<String>>(0))
            .map_err(sqlite_error)?;

        for row in rows {
            let value = row.map_err(sqlite_error)?;
            let Some(value) = value else {
                continue;
            };

            let value = value.trim();
            if value.is_empty() {
                continue;
            }

            values.insert(value.to_string());
        }

        Ok(())
    }

    fn delete_target_queue_messages(
        transaction: &Transaction<'_>,
        queue_messages: &[TargetQueueMessage],
    ) -> io::Result<()> {
        let mut statement = transaction
            .prepare("DELETE FROM worker_queue_messages WHERE id = ?")
            .map_err(sqlite_error)?;
        for queue_message in queue_messages {
            statement
                .execute([queue_message.id])
                .map_err(sqlite_error)?;
        }

        Ok(())
    }

    fn delete_target_coordination_leases(
        transaction: &Transaction<'_>,
        coordination_lease_names: &[String],
    ) -> io::Result<()> {
        let mut statement = transaction
            .prepare("DELETE FROM worker_coordination_leases WHERE name = ?")
            .map_err(sqlite_error)?;
        for coordination_lease_name in coordination_lease_names {
            statement
                .execute([coordination_lease_name])
                .map_err(sqlite_error)?;
        }

        Ok(())
    }

    fn delete_target_idempotency_keys(
        transaction: &Transaction<'_>,
        idempotency_keys: &[String],
    ) -> io::Result<()> {
        let mut statement = transaction
            .prepare("DELETE FROM worker_idempotency_keys WHERE idempotency_key = ?")
            .map_err(sqlite_error)?;
        for idempotency_key in idempotency_keys {
            statement
                .execute([idempotency_key])
                .map_err(sqlite_error)?;
        }

        Ok(())
    }

    fn matches_release_lock_name(name: &str, release_run_ids: &HashSet<i64>) -> bool {
        release_run_ids.iter().any(|release_run_id| {
            name == format!("release-run:{release_run_id}:dispatch")
                || name == format!("release-plan:{release_run_id}")
        })
    }

    fn matches_build_lock_name(name: &str, build_run_ids: &HashSet<i64>) -> bool {
        build_run_ids
            .iter()
            .any(|build_run_id| name == format!("build-run:{build_run_id}:dispatch"))
    }

    fn matches_publish_lock_name(name: &str, publish_run_ids: &HashSet<i64>) -> bool {
        publish_run_ids.iter().any(|publish_run_id| {
            name == format!("publish-run:{publish_run_id}:dispatch")
        })
    }

    fn matches_release_idempotency_key(
        key: &str,
        release_run_ids: &HashSet<i64>,
    ) -> bool {
        release_run_ids.iter().any(|release_run_id| {
            key == format!("release-run:{release_run_id}:queued")
        })
    }

    fn matches_build_idempotency_key(
        key: &str,
        build_run_ids: &HashSet<i64>,
    ) -> bool {
        build_run_ids.iter().any(|build_run_id| {
            key == format!("build-run:{build_run_id}:queued")
                || key.starts_with(&format!("build-run:{build_run_id}:"))
        })
    }

    fn matches_publish_idempotency_key(
        key: &str,
        publish_run_ids: &HashSet<i64>,
    ) -> bool {
        publish_run_ids.iter().any(|publish_run_id| {
            key == format!("publish-run:{publish_run_id}:queued")
                || key.starts_with(&format!("publish-run:{publish_run_id}:"))
        })
    }

    fn repository_exists(&self, repository_id: i64) -> io::Result<bool> {
        let connection = open_connection(&self.database_path)?;
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM repositories WHERE id = ?",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        Ok(count > 0)
    }

    fn repository_name(&self, repository_id: i64) -> io::Result<Option<String>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "SELECT name FROM repositories WHERE id = ?",
                [repository_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn release_run_id_by_repository_tag_and_source_identity(
        &self,
        repository_id: i64,
        git_tag: &str,
        source_identity: &str,
    ) -> io::Result<Option<i64>> {
        let git_tag = require_non_empty(git_tag, "git tag")?;
        let source_identity = require_non_empty(source_identity, "release source identity")?;
        let connection = open_connection(&self.database_path)?;
        let existing = connection
            .query_row(
                "
                SELECT id
                FROM release_runs
                WHERE repository_id = ?
                  AND git_tag = ?
                  AND source_identity = ?
                ",
                params![repository_id, git_tag, source_identity],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_error)?;
        if existing.is_some() {
            return Ok(existing);
        }

        if source_identity.starts_with(&format!("{RELEASE_SOURCE_KIND_MANAGED_TAG}:")) {
            return connection
                .query_row(
                    "
                    SELECT id
                    FROM release_runs
                    WHERE repository_id = ?
                      AND git_tag = ?
                      AND source_identity = ?
                    ",
                    params![repository_id, git_tag, RELEASE_SOURCE_KIND_MANAGED_TAG],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sqlite_error);
        }

        Ok(None)
    }

    fn repository_has_active_build_work(&self, repository_id: i64) -> io::Result<bool> {
        let connection = open_connection(&self.database_path)?;
        let count: i64 = connection
            .query_row(
                "
                SELECT COUNT(1)
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE rr.repository_id = ?
                  AND br.status IN (?, ?)
                ",
                params![
                    repository_id,
                    BuildStatus::Queued.as_str(),
                    BuildStatus::Running.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        Ok(count > 0)
    }

    fn reject_if_repository_build_work_active(&self, repository_id: i64) -> io::Result<()> {
        if !self.repository_has_active_build_work(repository_id)? {
            return Ok(());
        }

        let repository_name = self
            .repository_name(repository_id)?
            .unwrap_or_else(|| format!("repository-{repository_id}"));
        Err(io::Error::new(
            ErrorKind::WouldBlock,
            format!(
                "repository already has queued or running build work for repository {repository_name:?}"
            ),
        ))
    }

    fn list_enabled_build_targets_for_planning(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<Vec<BuildTargetPlanningState>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id,
                                             COALESCE(build_kind, ''),
                                             COALESCE(contract_json, ''),
                                             runner_type
                FROM build_targets
                WHERE repository_id = ?
                  AND enabled = 1
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([repository_id], |row| {
                Ok(BuildTargetPlanningState {
                    id: row.get(0)?,
                    build_kind: parse_build_kind_sql(1, row.get::<_, String>(1)?)?,
                    contract_json: row.get(2)?,
                    runner_type: row.get(3)?,
                })
            })
            .map_err(sqlite_error)?;

        let mut targets = Vec::new();
        for row in rows {
            targets.push(row.map_err(sqlite_error)?);
        }

        Ok(targets)
    }

    fn list_automation_queue_snapshots(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        now: i64,
    ) -> io::Result<Vec<AutomationQueueSnapshot>> {
        let mut queue_messages = HashMap::from([
            (
                BUILD_RUN_QUEUE_NAME.to_owned(),
                AutomationQueueSnapshot {
                    queue_name: BUILD_RUN_QUEUE_NAME.to_owned(),
                    ready_count: 0,
                    leased_count: 0,
                },
            ),
            (
                PUBLISH_RUN_QUEUE_NAME.to_owned(),
                AutomationQueueSnapshot {
                    queue_name: PUBLISH_RUN_QUEUE_NAME.to_owned(),
                    ready_count: 0,
                    leased_count: 0,
                },
            ),
            (
                RELEASE_RUN_QUEUE_NAME.to_owned(),
                AutomationQueueSnapshot {
                    queue_name: RELEASE_RUN_QUEUE_NAME.to_owned(),
                    ready_count: 0,
                    leased_count: 0,
                },
            ),
        ]);

        let mut statement = transaction
            .prepare(
                "
                SELECT queue_name,
                       SUM(
                           CASE
                               WHEN leased_by IS NULL
                                 OR lease_expires_at_unix_millis IS NULL
                                 OR lease_expires_at_unix_millis <= ?
                               THEN 1
                               ELSE 0
                           END
                       ) AS ready_count,
                       SUM(
                           CASE
                               WHEN leased_by IS NOT NULL
                                 AND lease_expires_at_unix_millis IS NOT NULL
                                 AND lease_expires_at_unix_millis > ?
                               THEN 1
                               ELSE 0
                           END
                       ) AS leased_count
                FROM worker_queue_messages
                GROUP BY queue_name
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(params![now, now], |row| {
                Ok(AutomationQueueSnapshot {
                    queue_name: row.get(0)?,
                    ready_count: row.get(1)?,
                    leased_count: row.get(2)?,
                })
            })
            .map_err(sqlite_error)?;

        for row in rows {
            let snapshot = row.map_err(sqlite_error)?;
            queue_messages.insert(snapshot.queue_name.clone(), snapshot);
        }

        let mut queue_names = queue_messages.keys().cloned().collect::<Vec<_>>();
        queue_names.sort();

        let mut ordered = Vec::with_capacity(queue_names.len());
        for queue_name in queue_names {
            if let Some(snapshot) = queue_messages.remove(&queue_name) {
                ordered.push(snapshot);
            }
        }

        Ok(ordered)
    }

    fn list_automation_coordination_leases(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        now: i64,
    ) -> io::Result<Vec<AutomationCoordinationLeaseSnapshot>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT name,
                       lease_expires_at_unix_millis
                FROM worker_coordination_leases
                WHERE lease_expires_at_unix_millis > ?
                ORDER BY name ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([now], |row| {
                Ok(AutomationCoordinationLeaseSnapshot {
                    name: row.get(0)?,
                    lease_expires_at_unix_millis: row.get(1)?,
                })
            })
            .map_err(sqlite_error)?;

        let mut leases = Vec::new();
        for row in rows {
            leases.push(row.map_err(sqlite_error)?);
        }

        Ok(leases)
    }

    fn list_automation_repositories(
        &self,
        transaction: &rusqlite::Transaction<'_>,
    ) -> io::Result<Vec<AutomationRepositoryRow>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id,
                       name,
                       enabled,
                       polling_interval_seconds,
                       last_seen_tag
                FROM repositories
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AutomationRepositoryRow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    enabled: row.get::<_, i64>(2)? != 0,
                    polling_interval_seconds: row.get(3)?,
                    last_seen_tag: normalize_optional_string(row.get(4)?),
                })
            })
            .map_err(sqlite_error)?;

        let mut repositories = Vec::new();
        for row in rows {
            repositories.push(row.map_err(sqlite_error)?);
        }

        Ok(repositories)
    }

    fn count_enabled_build_targets(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<i64> {
        transaction
            .query_row(
                "
                SELECT COUNT(1)
                FROM build_targets
                WHERE repository_id = ?
                  AND enabled = 1
                ",
                [repository_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)
    }

    fn list_queued_releases_by_repository(
        &self,
        repository_id: i64,
    ) -> io::Result<Vec<ReleaseRunRecord>> {
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(sqlite_error)?;
        let releases = self.list_queued_releases_by_repository_in_transaction(
            &transaction,
            repository_id,
        )?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(releases)
    }

    fn list_queued_releases_by_repository_in_transaction(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<Vec<ReleaseRunRecord>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id,
                       repository_id,
                       git_tag,
                       git_commit,
                       trigger_source,
                       trigger_rule_id,
                       source_metadata_json,
                      source_identity,
                      engine_version,
                       status,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM release_runs
                WHERE repository_id = ?
                  AND status = ?
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![repository_id, ReleaseStatus::Queued.as_str()],
                scan_release_run_record,
            )
            .map_err(sqlite_error)?;

        let mut releases = Vec::new();
        for row in rows {
            releases.push(row.map_err(sqlite_error)?);
        }

        Ok(releases)
    }

    fn list_publish_runs_by_release(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Vec<PublishRunRecord>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id,
                       release_run_id,
                       build_run_id,
                       publish_target_id,
                       artifact_id,
                       status,
                      destination_ref,
                      execution_contract_json,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM publish_runs
                WHERE release_run_id = ?
                ORDER BY publish_target_id ASC, id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([release_run_id], scan_publish_run_record)
            .map_err(sqlite_error)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row.map_err(sqlite_error)?);
        }

        Ok(runs)
    }

    fn list_process_feed_build_run_contexts(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        release_run_id: i64,
        repository_engine_kind: &str,
    ) -> io::Result<HashMap<i64, ProcessFeedBuildRunContext>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT br.id,
                       COALESCE(bt.name, ''),
                       COALESCE(bt.build_kind, ''),
                       COALESCE(bt.contract_json, '')
                FROM build_runs br
                INNER JOIN build_targets bt ON bt.id = br.build_target_id
                WHERE br.release_run_id = ?
                ORDER BY br.id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement.query([release_run_id]).map_err(sqlite_error)?;
        let mut contexts = HashMap::new();

        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let build_run_id = row.get::<_, i64>(0).map_err(sqlite_error)?;
            let target_name = row
                .get::<_, String>(1)
                .map_err(sqlite_error)?
                .trim()
                .to_owned();
            let build_kind = row.get::<_, String>(2).map_err(sqlite_error)?;
            let contract_json = row.get::<_, String>(3).map_err(sqlite_error)?;
            let unity_target_platform = resolve_build_target_read_model_projection(
                repository_engine_kind,
                build_kind.trim(),
                contract_json.as_str(),
            )
            .map(|projection| projection.unity_target_platform)
            .unwrap_or_default();

            contexts.insert(
                build_run_id,
                ProcessFeedBuildRunContext {
                    target_name,
                    unity_target_platform,
                },
            );
        }

        Ok(contexts)
    }

    fn list_process_feed_publish_run_contexts(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<HashMap<i64, ProcessFeedPublishRunContext>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT pr.id,
                       COALESCE(pt.name, ''),
                       a.name
                FROM publish_runs pr
                INNER JOIN publish_targets pt ON pt.id = pr.publish_target_id
                LEFT JOIN artifacts a ON a.id = pr.artifact_id
                WHERE pr.release_run_id = ?
                ORDER BY pr.id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let mut rows = statement.query([release_run_id]).map_err(sqlite_error)?;
        let mut contexts = HashMap::new();

        while let Some(row) = rows.next().map_err(sqlite_error)? {
            let publish_run_id = row.get::<_, i64>(0).map_err(sqlite_error)?;
            let publish_target_name = row
                .get::<_, String>(1)
                .map_err(sqlite_error)?
                .trim()
                .to_owned();
            let artifact_name = normalize_optional_string(
                row.get::<_, Option<String>>(2).map_err(sqlite_error)?,
            );

            contexts.insert(
                publish_run_id,
                ProcessFeedPublishRunContext {
                    publish_target_name,
                    artifact_name,
                },
            );
        }

        Ok(contexts)
    }

    fn list_repository_automation_releases(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        repository_id: i64,
    ) -> io::Result<Vec<ReleaseAutomationStatus>> {
        let releases = self.list_queued_releases_by_repository_in_transaction(
            transaction,
            repository_id,
        )?;
        let mut release_queue = Vec::new();

        for release in releases {
            let build_runs = self.list_build_runs_by_release(transaction, release.id)?;
            let publish_runs = self.list_publish_runs_by_release(transaction, release.id)?;

            if !release_requires_automation_attention(&build_runs, &publish_runs) {
                continue;
            }

            release_queue.push(summarize_release_automation_status(
                &release,
                &build_runs,
                &publish_runs,
            ));
        }

        Ok(release_queue)
    }

    fn list_build_runs_by_release(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Vec<BuildRunRecord>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id,
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
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM build_runs
                WHERE release_run_id = ?
                ORDER BY build_target_id ASC, id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map([release_run_id], scan_build_run_record)
            .map_err(sqlite_error)?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(row.map_err(sqlite_error)?);
        }

        Ok(runs)
    }

    fn list_open_release_run_ids(
        &self,
        transaction: &Transaction<'_>,
    ) -> io::Result<Vec<i64>> {
        let mut statement = transaction
            .prepare(
                "
                SELECT id
                FROM release_runs
                WHERE status IN (?, ?, ?)
                ORDER BY id ASC
                ",
            )
            .map_err(sqlite_error)?;
        let rows = statement
            .query_map(
                params![
                    ReleaseStatus::Detected.as_str(),
                    ReleaseStatus::Queued.as_str(),
                    ReleaseStatus::Running.as_str(),
                ],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sqlite_error)?;

        let mut release_run_ids = Vec::new();
        for row in rows {
            release_run_ids.push(row.map_err(sqlite_error)?);
        }

        Ok(release_run_ids)
    }

    fn reconcile_release_run_status_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<()> {
        let Some(release) = self.load_release_run_record_in_transaction(transaction, release_run_id)?
        else {
            return Ok(());
        };
        let build_runs = self.list_build_runs_by_release(transaction, release_run_id)?;
        let publish_runs = self.list_publish_runs_by_release(transaction, release_run_id)?;
        let Some(reconciliation) =
            derive_release_run_reconciliation(&release, &build_runs, &publish_runs)
        else {
            return Ok(());
        };

        if release.status == reconciliation.status
            && release.started_at == reconciliation.started_at
            && release.finished_at == reconciliation.finished_at
            && release.error_message == reconciliation.error_message
        {
            return Ok(());
        }

        transaction
            .execute(
                "
                UPDATE release_runs
                SET status = ?,
                    started_at = ?,
                    finished_at = ?,
                    error_message = ?,
                    updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                ",
                params![
                    reconciliation.status,
                    reconciliation.started_at.as_deref(),
                    reconciliation.finished_at.as_deref(),
                    reconciliation.error_message.as_deref(),
                    release_run_id,
                ],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    fn list_build_runs_by_release_readonly(
        &self,
        release_run_id: i64,
    ) -> io::Result<Vec<BuildRunRecord>> {
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(sqlite_error)?;
        let runs = self.list_build_runs_by_release(&transaction, release_run_id)?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(runs)
    }

    fn build_run_count_for_release(&self, release_run_id: i64) -> io::Result<i64> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE release_run_id = ?",
                [release_run_id],
                |row| row.get(0),
            )
            .map_err(sqlite_error)
    }

    fn dispatch_next_build_run_for_release(&self, release_run_id: i64) -> io::Result<()> {
        let Some(build_run_id) = self.next_queued_build_run_id_for_release(release_run_id)? else {
            return Ok(());
        };

        match self.dispatch_build_run(build_run_id)? {
            QueueDispatchOutcome::Enqueued | QueueDispatchOutcome::AlreadyClaimed => Ok(()),
            QueueDispatchOutcome::InProgress => Err(io::Error::new(
                ErrorKind::WouldBlock,
                format!("build run {build_run_id} dispatch is already in progress"),
            )),
        }
    }

    fn advance_release_after_terminal_build(&self, release_run_id: i64) -> io::Result<()> {
        let build_runs = self.list_build_runs_by_release_readonly(release_run_id)?;
        if build_runs
            .iter()
            .any(|run| run.status == BuildStatus::Running.as_str())
        {
            return Ok(());
        }

        if build_runs
            .iter()
            .any(|run| run.status == BuildStatus::Queued.as_str())
        {
            return self.dispatch_next_build_run_for_release(release_run_id);
        }

        for build_run in build_runs
            .iter()
            .filter(|run| run.status == BuildStatus::Succeeded.as_str())
        {
            if self.list_artifacts_by_build_run(build_run.id)?.is_empty() {
                continue;
            }

            for publish_run in self.plan_build_publish_runs(build_run.id)? {
                if publish_run.status != PublishStatus::Queued.as_str() {
                    continue;
                }

                match self.dispatch_publish_run(publish_run.id)? {
                    QueueDispatchOutcome::Enqueued | QueueDispatchOutcome::AlreadyClaimed => {}
                    QueueDispatchOutcome::InProgress => {
                        return Err(io::Error::new(
                            ErrorKind::WouldBlock,
                            format!(
                                "publish run {} dispatch is already in progress",
                                publish_run.id
                            ),
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    fn next_queued_build_run_id_for_release(
        &self,
        release_run_id: i64,
    ) -> io::Result<Option<i64>> {
        let mut connection = open_connection(&self.database_path)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(sqlite_error)?;
        let build_run_id =
            self.next_queued_build_run_id_in_transaction(&transaction, release_run_id)?;
        transaction.commit().map_err(sqlite_error)?;

        Ok(build_run_id)
    }

    fn next_queued_build_run_id_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Option<i64>> {
        transaction
            .query_row(
                "
                SELECT id
                FROM build_runs
                WHERE release_run_id = ?
                  AND status = ?
                ORDER BY build_target_id ASC, id ASC
                LIMIT 1
                ",
                params![release_run_id, BuildStatus::Queued.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn running_build_run_id_in_transaction(
        &self,
        transaction: &Transaction<'_>,
        release_run_id: i64,
    ) -> io::Result<Option<i64>> {
        transaction
            .query_row(
                "
                SELECT id
                FROM build_runs
                WHERE release_run_id = ?
                  AND status = ?
                ORDER BY build_target_id ASC, id ASC
                LIMIT 1
                ",
                params![release_run_id, BuildStatus::Running.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_build_run_dispatch_state(
        &self,
        build_run_id: i64,
    ) -> io::Result<Option<BuildRunDispatchState>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT release_run_id,
                       build_target_id,
                      engine_version,
                       image_ref,
                       status,
                       created_at
                FROM build_runs
                WHERE id = ?
                ",
                [build_run_id],
                |row| {
                    let engine_version = row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    let image_ref = row
                        .get::<_, Option<String>>(3)?
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    if engine_version.is_empty() || image_ref.is_empty() {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(io::Error::new(
                                ErrorKind::InvalidInput,
                                format!(
                                    "build run {build_run_id} is missing planned image metadata"
                                ),
                            )),
                        ));
                    }

                    Ok(BuildRunDispatchState {
                        job: BuildDispatchJob {
                            build_run_id,
                            release_run_id: row.get(0)?,
                            build_target_id: row.get(1)?,
                            engine_version,
                            image_ref,
                        },
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn load_publish_run_dispatch_state(
        &self,
        publish_run_id: i64,
    ) -> io::Result<Option<PublishRunDispatchState>> {
        let connection = open_connection(&self.database_path)?;
        connection
            .query_row(
                "
                SELECT release_run_id,
                       build_run_id,
                       publish_target_id,
                       artifact_id,
                       status,
                       created_at
                FROM publish_runs
                WHERE id = ?
                ",
                [publish_run_id],
                |row| {
                    let artifact_id = row.get::<_, Option<i64>>(3)?;
                    if artifact_id.is_some_and(|artifact_id| artifact_id <= 0) {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Integer,
                            Box::new(io::Error::new(
                                ErrorKind::InvalidInput,
                                format!(
                                    "publish run {publish_run_id} has an invalid artifact reference"
                                ),
                            )),
                        ));
                    }

                    Ok(PublishRunDispatchState {
                        job: PublishDispatchJob {
                            publish_run_id,
                            release_run_id: row.get(0)?,
                            build_run_id: row.get(1)?,
                            publish_target_id: row.get(2)?,
                            artifact_id,
                        },
                        status: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn repository_release_lane_available(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        repository_id: i64,
        release_run_id: i64,
        max_active_releases_per_repository: u32,
    ) -> io::Result<bool> {
        let blocking_prior_releases: u32 = transaction
            .query_row(
                "
                SELECT COUNT(DISTINCT br.release_run_id)
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE rr.repository_id = ?
                  AND br.release_run_id < ?
                  AND br.status IN (?, ?)
                ",
                params![
                    repository_id,
                    release_run_id,
                    BuildStatus::Queued.as_str(),
                    BuildStatus::Running.as_str(),
                ],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;
        if blocking_prior_releases >= max_active_releases_per_repository {
            return Ok(false);
        }

        let active_other_releases: u32 = transaction
            .query_row(
                "
                SELECT COUNT(DISTINCT br.release_run_id)
                FROM build_runs br
                JOIN release_runs rr ON rr.id = br.release_run_id
                WHERE rr.repository_id = ?
                  AND br.status = ?
                  AND br.release_run_id != ?
                ",
                params![
                    repository_id,
                    BuildStatus::Running.as_str(),
                    release_run_id,
                ],
                |row| row.get(0),
            )
            .map_err(sqlite_error)?;

        Ok(active_other_releases < max_active_releases_per_repository)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExistingRepositoryProjectBuildTarget {
    id: i64,
    name: String,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveRepositoryProjectBuildTarget {
    id: i64,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExistingRepositoryProjectPublishTarget {
    id: i64,
    name: String,
    enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExistingRepositoryProjectPublishBinding {
    build_target_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedRepositoryProjectBuildTargetUpdate {
    existing_target_id: Option<i64>,
    target: UpdateRepositoryProjectBuildTargetInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedRepositoryProjectPublishTargetUpdate {
    existing_target_id: Option<i64>,
    target: UpdateRepositoryProjectPublishTargetInput,
}

fn sync_repository_project_build_targets(
    transaction: &Transaction<'_>,
    repository_id: i64,
    repository_engine_kind: &str,
    build_targets: Vec<UpdateRepositoryProjectBuildTargetInput>,
) -> io::Result<()> {
    let existing_targets = list_repository_project_build_targets(transaction, repository_id)?;
    let existing_by_id = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.id, target))
        .collect::<HashMap<_, _>>();
    let existing_by_name = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.name.to_ascii_lowercase(), target))
        .collect::<HashMap<_, _>>();

    let mut claimed_existing_ids = HashSet::new();
    let mut planned_targets = Vec::with_capacity(build_targets.len());

    for target in build_targets {
        let normalized_name = target.name.to_ascii_lowercase();
        let existing_target_id = if let Some(build_target_id) = target.build_target_id {
            let Some(existing_target) = existing_by_id.get(&build_target_id) else {
                return Err(not_found_error(format!(
                    "build target {build_target_id} was not found for repository {repository_id}"
                )));
            };

            if !claimed_existing_ids.insert(build_target_id) {
                return Err(invalid_input_error(format!(
                    "build target {build_target_id} was provided more than once"
                )));
            }

            if let Some(conflicting_target) = existing_by_name.get(&normalized_name) {
                if conflicting_target.id != existing_target.id
                    && !claimed_existing_ids.contains(&conflicting_target.id)
                {
                    return Err(invalid_input_error(format!(
                        "repository project build target name {:?} is already reserved by another target",
                        target.name
                    )));
                }
            }

            Some(existing_target.id)
        } else if let Some(existing_target) = existing_by_name.get(&normalized_name) {
            if claimed_existing_ids.contains(&existing_target.id) {
                None
            } else {
                claimed_existing_ids.insert(existing_target.id);
                Some(existing_target.id)
            }
        } else {
            None
        };

        planned_targets.push(PlannedRepositoryProjectBuildTargetUpdate {
            existing_target_id,
            target,
        });
    }

    let planned_existing_ids = planned_targets
        .iter()
        .filter_map(|target| target.existing_target_id)
        .collect::<HashSet<_>>();

    for planned_target in &planned_targets {
        let normalized_name = planned_target.target.name.to_ascii_lowercase();
        if let Some(existing_target_id) = planned_target.existing_target_id {
            let existing_target = existing_by_id
                .get(&existing_target_id)
                .expect("planned target ids must resolve against repository targets");

            if !existing_target
                .name
                .eq_ignore_ascii_case(&planned_target.target.name)
            {
                rename_repository_project_build_target(
                    transaction,
                    existing_target_id,
                    &temporary_repository_project_build_target_name(existing_target_id),
                )?;
            }

            continue;
        }

        if let Some(conflicting_target) = existing_by_name.get(&normalized_name) {
            if !planned_existing_ids.contains(&conflicting_target.id) {
                return Err(invalid_input_error(format!(
                    "repository project build target name {:?} is already reserved by an inactive target",
                    planned_target.target.name
                )));
            }
        }
    }

    let mut retained_target_ids = HashSet::new();
    for planned_target in planned_targets {
        let target_id = if let Some(existing_target_id) = planned_target.existing_target_id {
            update_repository_project_build_target(
                transaction,
                existing_target_id,
                repository_engine_kind,
                &planned_target.target,
            )?;
            existing_target_id
        } else {
            create_repository_project_build_target(
                transaction,
                repository_id,
                repository_engine_kind,
                &planned_target.target,
            )?
        };

        retained_target_ids.insert(target_id);
    }

    for existing_target in existing_targets {
        if retained_target_ids.contains(&existing_target.id) || !existing_target.enabled {
            continue;
        }

        disable_repository_project_build_target(transaction, existing_target.id)?;
    }

    Ok(())
}

fn promote_create_repository_project_publish_target_input(
    input: CreateRepositoryProjectPublishTargetInput,
) -> UpdateRepositoryProjectPublishTargetInput {
    UpdateRepositoryProjectPublishTargetInput {
        publish_target_id: None,
        name: input.name,
        kind: input.kind,
        enabled: input.enabled,
        config_json: input.config_json,
        credentials_id: input.credentials_id,
        bindings: input
            .bindings
            .into_iter()
            .map(|binding| UpdateRepositoryProjectPublishBindingInput {
                build_target_id: None,
                build_target_name: binding.build_target_name,
                enabled: binding.enabled,
                options_json: binding.options_json,
            })
            .collect(),
    }
}

fn sync_repository_project_publish_targets(
    transaction: &Transaction<'_>,
    repository_id: i64,
    publish_targets: Vec<UpdateRepositoryProjectPublishTargetInput>,
    build_targets: &[ActiveRepositoryProjectBuildTarget],
) -> io::Result<()> {
    let build_targets_by_id = build_targets
        .iter()
        .cloned()
        .map(|target| (target.id, target))
        .collect::<HashMap<_, _>>();
    let build_targets_by_name = build_targets
        .iter()
        .cloned()
        .map(|target| (target.name.to_ascii_lowercase(), target))
        .collect::<HashMap<_, _>>();

    validate_repository_project_publish_targets_against_build_targets(
        &publish_targets,
        &build_targets_by_id,
        &build_targets_by_name,
    )?;

    let existing_targets = list_repository_project_publish_targets(transaction, repository_id)?;
    let existing_by_id = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.id, target))
        .collect::<HashMap<_, _>>();
    let existing_by_name = existing_targets
        .iter()
        .cloned()
        .map(|target| (target.name.to_ascii_lowercase(), target))
        .collect::<HashMap<_, _>>();

    let mut claimed_existing_ids = HashSet::new();
    let mut planned_targets = Vec::with_capacity(publish_targets.len());

    for target in publish_targets {
        let normalized_name = target.name.to_ascii_lowercase();
        let existing_target_id = if let Some(publish_target_id) = target.publish_target_id {
            let Some(existing_target) = existing_by_id.get(&publish_target_id) else {
                return Err(not_found_error(format!(
                    "publish target {publish_target_id} was not found for repository {repository_id}"
                )));
            };

            if !claimed_existing_ids.insert(publish_target_id) {
                return Err(invalid_input_error(format!(
                    "publish target {publish_target_id} was provided more than once"
                )));
            }

            if let Some(conflicting_target) = existing_by_name.get(&normalized_name) {
                if conflicting_target.id != existing_target.id
                    && !claimed_existing_ids.contains(&conflicting_target.id)
                {
                    return Err(invalid_input_error(format!(
                        "repository project publish target name {:?} is already reserved by another target",
                        target.name
                    )));
                }
            }

            Some(existing_target.id)
        } else if let Some(existing_target) = existing_by_name.get(&normalized_name) {
            if claimed_existing_ids.contains(&existing_target.id) {
                None
            } else {
                claimed_existing_ids.insert(existing_target.id);
                Some(existing_target.id)
            }
        } else {
            None
        };

        planned_targets.push(PlannedRepositoryProjectPublishTargetUpdate {
            existing_target_id,
            target,
        });
    }

    let planned_existing_ids = planned_targets
        .iter()
        .filter_map(|target| target.existing_target_id)
        .collect::<HashSet<_>>();

    for planned_target in &planned_targets {
        let normalized_name = planned_target.target.name.to_ascii_lowercase();
        if let Some(existing_target_id) = planned_target.existing_target_id {
            let existing_target = existing_by_id
                .get(&existing_target_id)
                .expect("planned publish target ids must resolve against repository targets");

            if !existing_target
                .name
                .eq_ignore_ascii_case(&planned_target.target.name)
            {
                rename_repository_project_publish_target(
                    transaction,
                    existing_target_id,
                    &temporary_repository_project_publish_target_name(existing_target_id),
                )?;
            }

            continue;
        }

        if let Some(conflicting_target) = existing_by_name.get(&normalized_name) {
            if !planned_existing_ids.contains(&conflicting_target.id) {
                return Err(invalid_input_error(format!(
                    "repository project publish target name {:?} is already reserved by an inactive target",
                    planned_target.target.name
                )));
            }
        }
    }

    let mut retained_target_ids = HashSet::new();
    for planned_target in planned_targets {
        let publish_target_id = if let Some(existing_target_id) = planned_target.existing_target_id {
            update_repository_project_publish_target(
                transaction,
                existing_target_id,
                &planned_target.target,
            )?;
            existing_target_id
        } else {
            create_repository_project_publish_target(
                transaction,
                repository_id,
                &planned_target.target,
            )?
        };

        sync_repository_project_publish_target_bindings(
            transaction,
            publish_target_id,
            &planned_target.target.bindings,
            &build_targets_by_id,
            &build_targets_by_name,
        )?;
        retained_target_ids.insert(publish_target_id);
    }

    for existing_target in existing_targets {
        if retained_target_ids.contains(&existing_target.id) || !existing_target.enabled {
            continue;
        }

        delete_repository_project_publish_target_bindings(transaction, existing_target.id)?;
        if publish_target_has_history(transaction, existing_target.id)? {
            disable_repository_project_publish_target(transaction, existing_target.id)?;
        } else {
            delete_repository_project_publish_target(transaction, existing_target.id)?;
        }
    }

    Ok(())
}

fn validate_repository_project_publish_targets_against_build_targets(
    publish_targets: &[UpdateRepositoryProjectPublishTargetInput],
    build_targets_by_id: &HashMap<i64, ActiveRepositoryProjectBuildTarget>,
    build_targets_by_name: &HashMap<String, ActiveRepositoryProjectBuildTarget>,
) -> io::Result<()> {
    let mut consuming_binding_count_by_build_target = HashMap::<i64, usize>::new();

    for publish_target in publish_targets {
        for binding in &publish_target.bindings {
            let build_target = resolve_repository_project_publish_binding_build_target(
                binding,
                build_targets_by_id,
                build_targets_by_name,
            )?;

            if !binding.enabled {
                continue;
            }

            if classify_publish_binding_consumption(
                publish_target.kind.as_str(),
                binding.options_json.as_str(),
            )? != PublishBindingConsumption::Consuming
            {
                continue;
            }

            let count = consuming_binding_count_by_build_target
                .entry(build_target.id)
                .or_default();
            *count += 1;
            if *count > 1 {
                return Err(invalid_input_error(format!(
                    "build target {:?} cannot enable more than one consuming publish binding",
                    build_target.name
                )));
            }
        }
    }

    Ok(())
}

fn resolve_repository_project_publish_binding_build_target<'a>(
    binding: &UpdateRepositoryProjectPublishBindingInput,
    build_targets_by_id: &'a HashMap<i64, ActiveRepositoryProjectBuildTarget>,
    build_targets_by_name: &'a HashMap<String, ActiveRepositoryProjectBuildTarget>,
) -> io::Result<&'a ActiveRepositoryProjectBuildTarget> {
    if let Some(build_target_id) = binding.build_target_id {
        let Some(build_target) = build_targets_by_id.get(&build_target_id) else {
            return Err(not_found_error(format!(
                "publish binding build target {build_target_id} was not found for this repository project"
            )));
        };

        if !build_target
            .name
            .eq_ignore_ascii_case(binding.build_target_name.as_str())
        {
            return Err(invalid_input_error(format!(
                "publish binding build target {:?} does not match build target {build_target_id}",
                binding.build_target_name
            )));
        }

        return Ok(build_target);
    }

    let normalized_name = binding.build_target_name.to_ascii_lowercase();
    build_targets_by_name.get(&normalized_name).ok_or_else(|| {
        not_found_error(format!(
            "publish binding build target {:?} was not found for this repository project",
            binding.build_target_name
        ))
    })
}

fn list_active_repository_project_build_targets(
    connection: &Connection,
    repository_id: i64,
) -> io::Result<Vec<ActiveRepositoryProjectBuildTarget>> {
    Ok(list_repository_project_build_targets(connection, repository_id)?
        .into_iter()
        .filter(|target| target.enabled)
        .map(|target| ActiveRepositoryProjectBuildTarget {
            id: target.id,
            name: target.name,
        })
        .collect())
}

fn list_repository_project_publish_targets(
    connection: &Connection,
    repository_id: i64,
) -> io::Result<Vec<ExistingRepositoryProjectPublishTarget>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, name, enabled
            FROM publish_targets
            WHERE repository_id = ?
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([repository_id], |row| {
            Ok(ExistingRepositoryProjectPublishTarget {
                id: row.get(0)?,
                name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(sqlite_error)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)
}

fn temporary_repository_project_publish_target_name(publish_target_id: i64) -> String {
    format!("__hgp_publish_target_update_{publish_target_id}")
}

fn rename_repository_project_publish_target(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    name: &str,
) -> io::Result<()> {
    let updated = transaction
        .execute(
            "
            UPDATE publish_targets
            SET name = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![name.trim(), publish_target_id],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "publish target {publish_target_id} was not found"
        )));
    }

    Ok(())
}

fn create_repository_project_publish_target(
    transaction: &Transaction<'_>,
    repository_id: i64,
    target: &UpdateRepositoryProjectPublishTargetInput,
) -> io::Result<i64> {
    validate_repository_project_publish_target_credentials_binding(
        transaction,
        target.credentials_id,
        target.kind.as_str(),
    )?;

    transaction
        .execute(
            "
            INSERT INTO publish_targets (
                repository_id,
                name,
                kind,
                credentials_id,
                enabled,
                config_json
            )
            VALUES (?, ?, ?, ?, ?, ?)
            ",
            params![
                repository_id,
                target.name.as_str(),
                target.kind.as_str(),
                target.credentials_id,
                target.enabled,
                target.config_json.as_str(),
            ],
        )
        .map_err(sqlite_error)?;

    Ok(transaction.last_insert_rowid())
}

fn update_repository_project_publish_target(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    target: &UpdateRepositoryProjectPublishTargetInput,
) -> io::Result<()> {
    validate_repository_project_publish_target_credentials_binding(
        transaction,
        target.credentials_id,
        target.kind.as_str(),
    )?;

    let updated = transaction
        .execute(
            "
            UPDATE publish_targets
            SET name = ?,
                kind = ?,
                credentials_id = ?,
                enabled = ?,
                config_json = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![
                target.name.as_str(),
                target.kind.as_str(),
                target.credentials_id,
                target.enabled,
                target.config_json.as_str(),
                publish_target_id,
            ],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "publish target {publish_target_id} was not found"
        )));
    }

    Ok(())
}

fn disable_repository_project_publish_target(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
) -> io::Result<()> {
    let updated = transaction
        .execute(
            "
            UPDATE publish_targets
            SET enabled = 0,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            [publish_target_id],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "publish target {publish_target_id} was not found"
        )));
    }

    Ok(())
}

fn delete_repository_project_publish_target(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
) -> io::Result<()> {
    transaction
        .execute("DELETE FROM publish_targets WHERE id = ?", [publish_target_id])
        .map_err(sqlite_error)?;

    Ok(())
}

fn publish_target_has_history(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
) -> io::Result<bool> {
    let exists = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM publish_runs WHERE publish_target_id = ?)",
            [publish_target_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;

    Ok(exists != 0)
}

fn sync_repository_project_publish_target_bindings(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    bindings: &[UpdateRepositoryProjectPublishBindingInput],
    build_targets_by_id: &HashMap<i64, ActiveRepositoryProjectBuildTarget>,
    build_targets_by_name: &HashMap<String, ActiveRepositoryProjectBuildTarget>,
) -> io::Result<()> {
    let existing_bindings = list_repository_project_publish_target_bindings(
        transaction,
        publish_target_id,
    )?;
    let existing_by_build_target_id = existing_bindings
        .iter()
        .cloned()
        .map(|binding| (binding.build_target_id, binding))
        .collect::<HashMap<_, _>>();

    let mut retained_build_target_ids = HashSet::new();
    for binding in bindings {
        let build_target = resolve_repository_project_publish_binding_build_target(
            binding,
            build_targets_by_id,
            build_targets_by_name,
        )?;

        if !retained_build_target_ids.insert(build_target.id) {
            return Err(invalid_input_error(format!(
                "publish target {publish_target_id} cannot bind build target {:?} more than once",
                build_target.name
            )));
        }

        if existing_by_build_target_id.contains_key(&build_target.id) {
            update_repository_project_publish_target_binding(
                transaction,
                publish_target_id,
                build_target.id,
                binding,
            )?;
        } else {
            create_repository_project_publish_target_binding(
                transaction,
                publish_target_id,
                build_target.id,
                binding,
            )?;
        }
    }

    for existing_binding in existing_bindings {
        if retained_build_target_ids.contains(&existing_binding.build_target_id) {
            continue;
        }

        delete_repository_project_publish_target_binding(
            transaction,
            publish_target_id,
            existing_binding.build_target_id,
        )?;
    }

    Ok(())
}

fn list_repository_project_publish_target_bindings(
    connection: &Connection,
    publish_target_id: i64,
) -> io::Result<Vec<ExistingRepositoryProjectPublishBinding>> {
    let mut statement = connection
        .prepare(
            "
            SELECT build_target_id
            FROM build_publish_bindings
            WHERE publish_target_id = ?
            ORDER BY build_target_id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([publish_target_id], |row| {
            Ok(ExistingRepositoryProjectPublishBinding {
                build_target_id: row.get(0)?,
            })
        })
        .map_err(sqlite_error)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)
}

fn create_repository_project_publish_target_binding(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    build_target_id: i64,
    binding: &UpdateRepositoryProjectPublishBindingInput,
) -> io::Result<()> {
    transaction
        .execute(
            "
            INSERT INTO build_publish_bindings (
                build_target_id,
                publish_target_id,
                enabled,
                options_json
            )
            VALUES (?, ?, ?, ?)
            ",
            params![
                build_target_id,
                publish_target_id,
                binding.enabled,
                binding.options_json.as_str(),
            ],
        )
        .map_err(sqlite_error)?;

    Ok(())
}

fn update_repository_project_publish_target_binding(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    build_target_id: i64,
    binding: &UpdateRepositoryProjectPublishBindingInput,
) -> io::Result<()> {
    let updated = transaction
        .execute(
            "
            UPDATE build_publish_bindings
            SET enabled = ?,
                options_json = ?
            WHERE build_target_id = ?
              AND publish_target_id = ?
            ",
            params![
                binding.enabled,
                binding.options_json.as_str(),
                build_target_id,
                publish_target_id,
            ],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "publish binding for build target {build_target_id} and publish target {publish_target_id} was not found"
        )));
    }

    Ok(())
}

fn delete_repository_project_publish_target_binding(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
    build_target_id: i64,
) -> io::Result<()> {
    transaction
        .execute(
            "
            DELETE FROM build_publish_bindings
            WHERE build_target_id = ?
              AND publish_target_id = ?
            ",
            params![build_target_id, publish_target_id],
        )
        .map_err(sqlite_error)?;

    Ok(())
}

fn delete_repository_project_publish_target_bindings(
    transaction: &Transaction<'_>,
    publish_target_id: i64,
) -> io::Result<()> {
    transaction
        .execute(
            "DELETE FROM build_publish_bindings WHERE publish_target_id = ?",
            [publish_target_id],
        )
        .map_err(sqlite_error)?;

    Ok(())
}

fn list_repository_project_build_targets(
    connection: &Connection,
    repository_id: i64,
) -> io::Result<Vec<ExistingRepositoryProjectBuildTarget>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id, name, enabled
            FROM build_targets
            WHERE repository_id = ?
            ORDER BY id
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([repository_id], |row| {
            Ok(ExistingRepositoryProjectBuildTarget {
                id: row.get(0)?,
                name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(sqlite_error)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sqlite_error)
}

fn temporary_repository_project_build_target_name(build_target_id: i64) -> String {
    format!("__hgp_target_update_{build_target_id}")
}

fn rename_repository_project_build_target(
    transaction: &Transaction<'_>,
    build_target_id: i64,
    name: &str,
) -> io::Result<()> {
    let updated = transaction
        .execute(
            "
            UPDATE build_targets
            SET name = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![name.trim(), build_target_id],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "build target {build_target_id} was not found"
        )));
    }

    Ok(())
}

fn create_repository_project_build_target(
    transaction: &Transaction<'_>,
    repository_id: i64,
    repository_engine_kind: &str,
    target: &UpdateRepositoryProjectBuildTargetInput,
) -> io::Result<i64> {
    project_repository_project_build_target_contract(
        repository_engine_kind,
        &target.build_kind,
        &target.contract_json,
    )?;

    transaction
        .execute(
            "
            INSERT INTO build_targets (
                repository_id,
                name,
                build_kind,
                runner_type,
                output_kind,
                output_path_template,
                timeout_seconds,
                enabled,
                contract_json,
                config_json
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ",
            params![
                repository_id,
                target.name.as_str(),
                target.build_kind.as_str(),
                target.runner_type.as_str(),
                target.output_kind.as_deref(),
                target.output_path_template.as_deref(),
                target.timeout_seconds,
                target.enabled,
                target.contract_json.as_str(),
                target.runner_config_json.as_str(),
            ],
        )
        .map_err(sqlite_error)?;

    Ok(transaction.last_insert_rowid())
}

fn update_repository_project_build_target(
    transaction: &Transaction<'_>,
    build_target_id: i64,
    repository_engine_kind: &str,
    target: &UpdateRepositoryProjectBuildTargetInput,
) -> io::Result<()> {
    project_repository_project_build_target_contract(
        repository_engine_kind,
        &target.build_kind,
        &target.contract_json,
    )?;

    let updated = transaction
        .execute(
            "
            UPDATE build_targets
            SET name = ?,
                build_kind = ?,
                runner_type = ?,
                output_kind = ?,
                output_path_template = ?,
                timeout_seconds = ?,
                enabled = ?,
                contract_json = ?,
                config_json = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            params![
                target.name.as_str(),
                target.build_kind.as_str(),
                target.runner_type.as_str(),
                target.output_kind.as_deref(),
                target.output_path_template.as_deref(),
                target.timeout_seconds,
                target.enabled,
                target.contract_json.as_str(),
                target.runner_config_json.as_str(),
                build_target_id,
            ],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "build target {build_target_id} was not found"
        )));
    }

    Ok(())
}

fn disable_repository_project_build_target(
    transaction: &Transaction<'_>,
    build_target_id: i64,
) -> io::Result<()> {
    let updated = transaction
        .execute(
            "
            UPDATE build_targets
            SET enabled = 0,
                updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            ",
            [build_target_id],
        )
        .map_err(sqlite_error)?;
    if updated == 0 {
        return Err(not_found_error(format!(
            "build target {build_target_id} was not found"
        )));
    }

    Ok(())
}

/// Opens SQLite, applies runtime pragmas, and runs pending migrations.
pub fn initialize_database(storage: &StorageLayout) -> io::Result<DatabaseBootstrapReport> {
    if let Some(parent) = storage.database_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut connection = open_connection(&storage.database_path)?;
    ensure_migration_ledger(&connection)?;
    let applied_migrations = apply_migrations(&mut connection)?;
    let pragmas = read_pragmas(&connection)?;

    Ok(DatabaseBootstrapReport {
        database_path: storage.database_path.clone(),
        busy_timeout_millis: pragmas.busy_timeout_millis,
        foreign_keys_enabled: pragmas.foreign_keys_enabled,
        journal_mode: pragmas.journal_mode,
        applied_migrations,
    })
}

/// Reconciles stale local queue leases, interrupted execution rows, and orphaned
/// host-native Unity processes after a restart.
pub fn recover_runtime_state(
    storage: &StorageLayout,
    interruption_kind: &str,
    interruption_message: &str,
) -> io::Result<RuntimeRecoveryReport> {
    let interruption_kind = require_non_empty(interruption_kind, "recovery interruption kind")?;
    let interruption_message =
        require_non_empty(interruption_message, "recovery interruption message")?;
    let mut connection = open_connection(&storage.database_path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sqlite_error)?;
    let interrupted_builds = list_interrupted_build_recovery_records(
        &transaction,
        interruption_kind.as_str(),
        interruption_message.as_str(),
    )?;

    let released_queue_message_leases = transaction
        .execute(
            "
            UPDATE worker_queue_messages
            SET leased_by = NULL,
                lease_token = NULL,
                lease_expires_at_unix_millis = NULL
            WHERE leased_by IS NOT NULL
               OR lease_token IS NOT NULL
               OR lease_expires_at_unix_millis IS NOT NULL
            ",
            [],
        )
        .map_err(sqlite_error)? as u64;
    let cleared_coordination_leases = transaction
        .execute("DELETE FROM worker_coordination_leases", [])
        .map_err(sqlite_error)? as u64;
    transaction
        .execute(
            "
            UPDATE build_run_steps
            SET status = ?,
                last_message = ?,
                heartbeat_at = CURRENT_TIMESTAMP,
                finished_at = COALESCE(finished_at, CURRENT_TIMESTAMP),
                error_message = ?,
                updated_at = CURRENT_TIMESTAMP
            WHERE build_run_id IN (
                SELECT id
                FROM build_runs
                WHERE status = ?
            )
              AND status = ?
            ",
            params![
                BuildStatus::Failed.as_str(),
                interruption_message,
                interruption_message,
                BuildStatus::Running.as_str(),
                BuildStatus::Running.as_str(),
            ],
        )
        .map_err(sqlite_error)?;
    let requeued_build_runs = transaction
        .execute(
            "
            UPDATE build_runs
            SET status = ?,
                workspace_path = NULL,
                log_path = NULL,
                artifact_root_path = NULL,
                current_stage_key = NULL,
                current_stage_label = NULL,
                current_stage_status = NULL,
                heartbeat_at = NULL,
                last_progress_message = NULL,
                started_at = NULL,
                finished_at = NULL,
                error_message = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE status = ?
            ",
            params![BuildStatus::Queued.as_str(), BuildStatus::Running.as_str()],
        )
        .map_err(sqlite_error)? as u64;
    let requeued_publish_runs = transaction
        .execute(
            "
            UPDATE publish_runs
            SET status = ?,
                destination_ref = NULL,
                started_at = NULL,
                finished_at = NULL,
                error_message = NULL,
                updated_at = CURRENT_TIMESTAMP
            WHERE status = ?
            ",
            params![PublishStatus::Queued.as_str(), PublishStatus::Running.as_str()],
        )
        .map_err(sqlite_error)? as u64;

    transaction.commit().map_err(sqlite_error)?;
    let orphan_build_process_report = terminate_interrupted_build_processes(&interrupted_builds);

    Ok(RuntimeRecoveryReport {
        released_queue_message_leases,
        cleared_coordination_leases,
        requeued_build_runs,
        requeued_publish_runs,
        terminated_orphan_build_processes: orphan_build_process_report.terminated_processes,
        orphan_build_process_errors: orphan_build_process_report.errors,
        interrupted_builds,
    })
}

fn list_interrupted_build_recovery_records(
    transaction: &Transaction<'_>,
    interruption_kind: &str,
    interruption_message: &str,
) -> io::Result<Vec<InterruptedBuildRecoveryRecord>> {
    let mut statement = transaction
        .prepare(
            "
            SELECT id,
                   workspace_path,
                   log_path
            FROM build_runs
            WHERE status = ?
              AND workspace_path IS NOT NULL
              AND TRIM(workspace_path) != ''
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map(params![BuildStatus::Running.as_str()], |row| {
            Ok(InterruptedBuildRecoveryRecord {
                build_run_id: row.get(0)?,
                workspace_path: row.get(1)?,
                log_path: row.get(2)?,
                interruption_kind: interruption_kind.to_owned(),
                interruption_message: interruption_message.to_owned(),
            })
        })
        .map_err(sqlite_error)?;

    rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_error)
}

fn terminate_interrupted_build_processes(
    interrupted_builds: &[InterruptedBuildRecoveryRecord],
) -> OrphanBuildProcessTerminationReport {
    if interrupted_builds.is_empty() {
        return OrphanBuildProcessTerminationReport::default();
    }

    let observed_processes = snapshot_observed_processes();
    let process_roots = select_orphan_build_process_roots(
        interrupted_builds,
        &observed_processes,
    );
    let mut report = OrphanBuildProcessTerminationReport::default();

    for pid in process_roots {
        match terminate_orphan_build_process_root(pid) {
            Ok(()) => report.terminated_processes += 1,
            Err(_) => report.errors += 1,
        }
    }

    report
}

fn snapshot_observed_processes() -> Vec<ObservedProcess> {
    let system = System::new_all();

    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let pid = sysinfo_pid_to_u32(*pid)?;
            let parent_pid = process.parent().and_then(sysinfo_pid_to_u32);
            let command_line = process
                .cmd()
                .iter()
                .map(|part| part.to_string())
                .collect::<Vec<_>>()
                .join(" ");

            Some(ObservedProcess {
                pid,
                parent_pid,
                name: process.name().to_string(),
                command_line,
            })
        })
        .collect()
}

fn sysinfo_pid_to_u32(pid: sysinfo::Pid) -> Option<u32> {
    pid.to_string().parse().ok()
}

fn select_orphan_build_process_roots(
    interrupted_builds: &[InterruptedBuildRecoveryRecord],
    processes: &[ObservedProcess],
) -> Vec<u32> {
    let process_lookup = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect::<HashMap<_, _>>();
    let mut matched_pids = HashSet::new();

    for build in interrupted_builds {
        let match_tokens = interrupted_build_process_match_tokens(build);
        if match_tokens.is_empty() {
            continue;
        }

        for process in processes {
            if !process_name_is_unity_like(&process.name) {
                continue;
            }

            let command_line = normalize_process_match_text(&process.command_line);
            if match_tokens
                .iter()
                .any(|token| !token.is_empty() && command_line.contains(token))
            {
                matched_pids.insert(process.pid);
            }
        }
    }

    let mut roots = matched_pids
        .iter()
        .copied()
        .filter(|pid| {
            !has_matched_ancestor(*pid, &matched_pids, &process_lookup)
        })
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots
}

fn interrupted_build_process_match_tokens(
    build: &InterruptedBuildRecoveryRecord,
) -> Vec<String> {
    let mut tokens = Vec::new();

    if !build.workspace_path.trim().is_empty() {
        tokens.push(normalize_process_match_text(&build.workspace_path));
    }

    if tokens.is_empty() {
        if let Some(log_path) = build.log_path.as_deref().map(str::trim) {
            if !log_path.is_empty() {
                tokens.push(normalize_process_match_text(log_path));
            }
        }
    }

    tokens
}

fn process_name_is_unity_like(name: &str) -> bool {
    normalize_process_match_text(name).contains("unity")
}

fn normalize_process_match_text(value: &str) -> String {
    if cfg!(windows) {
        value.to_ascii_lowercase().replace('/', "\\")
    } else {
        value.to_owned()
    }
}

fn has_matched_ancestor(
    pid: u32,
    matched_pids: &HashSet<u32>,
    process_lookup: &HashMap<u32, &ObservedProcess>,
) -> bool {
    let mut visited = HashSet::new();
    let mut current = process_lookup.get(&pid).and_then(|process| process.parent_pid);

    while let Some(parent_pid) = current {
        if !visited.insert(parent_pid) {
            break;
        }
        if matched_pids.contains(&parent_pid) {
            return true;
        }
        current = process_lookup
            .get(&parent_pid)
            .and_then(|process| process.parent_pid);
    }

    false
}

#[cfg(windows)]
fn terminate_orphan_build_process_root(pid: u32) -> io::Result<()> {
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "terminate orphan Unity process tree {pid}: taskkill exited with {status}",
    )))
}

#[cfg(not(windows))]
fn terminate_orphan_build_process_root(pid: u32) -> io::Result<()> {
    let system = System::new_all();
    let Some(process) = system.processes().iter().find_map(|(candidate, process)| {
        (sysinfo_pid_to_u32(*candidate) == Some(pid)).then_some(process)
    }) else {
        return Ok(());
    };

    if process.kill() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "terminate orphan Unity process {pid}: process.kill() returned false",
    )))
}

/// Opens a single SQLite connection primed with the runtime pragmas.
pub fn open_connection(database_path: &Path) -> io::Result<Connection> {
    let connection = Connection::open(database_path).map_err(sqlite_error)?;
    apply_pragmas(&connection)?;

    Ok(connection)
}

/// Lists one operator-facing page from the process feed using the default
/// archive scope without additional filters.
pub fn list_process_feed_page(
    storage: &StorageLayout,
    page: u32,
    page_size: u32,
) -> io::Result<ProcessFeedPage> {
    list_process_feed_page_filtered(
        storage,
        &ProcessFeedPageQuery {
            page,
            page_size,
            ..ProcessFeedPageQuery::default()
        },
    )
}

/// Lists one operator-facing page from the process feed using explicit scope,
/// status, and text filters.
pub fn list_process_feed_page_filtered(
    storage: &StorageLayout,
    query: &ProcessFeedPageQuery,
) -> io::Result<ProcessFeedPage> {
    let coordinator = LocalCoordinator::new(storage);
    coordinator.reconcile_release_run_statuses()?;

    let requested_page = query.page.max(1);
    let normalized_page_size = query.page_size.clamp(1, 50);
    let normalized_query = normalize_process_feed_search_query(query.query.as_deref());
    let mut connection = open_connection(&storage.database_path)?;
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Deferred)
        .map_err(sqlite_error)?;
    let generated_at = transaction
        .query_row(
            "SELECT STRFTIME('%Y-%m-%dT%H:%M:%SZ', 'now')",
            [],
            |row| row.get::<_, String>(0),
        )
        .map_err(sqlite_error)?;
    let page_rows = list_process_feed_release_rows(
        &transaction,
        query.scope,
        normalized_query.as_deref(),
    )?;
    let mut matching_items = Vec::with_capacity(page_rows.len());

    for row in page_rows {
        let repository_engine_kind = row.repository_engine_kind.clone();
        let build_runs = coordinator.list_build_runs_by_release(&transaction, row.release.id)?;
        let publish_runs = coordinator.list_publish_runs_by_release(&transaction, row.release.id)?;
        let build_run_contexts = coordinator.list_process_feed_build_run_contexts(
            &transaction,
            row.release.id,
            repository_engine_kind.as_str(),
        )?;
        let publish_run_contexts = coordinator
            .list_process_feed_publish_run_contexts(&transaction, row.release.id)?;
        let record = summarize_process_feed_record(
            row,
            &build_runs,
            &publish_runs,
            &build_run_contexts,
            &publish_run_contexts,
        );

        if !process_feed_record_matches_status(&record, query.status) {
            continue;
        }

        matching_items.push(record);
    }

    let total_items = matching_items.len() as i64;
    let total_pages = if total_items == 0 {
        0
    } else {
        ((total_items - 1) / i64::from(normalized_page_size) + 1) as u32
    };
    let effective_page = if total_pages == 0 {
        1
    } else {
        requested_page.min(total_pages)
    };
    let offset = i64::from(effective_page.saturating_sub(1)) * i64::from(normalized_page_size);
    let items = matching_items
        .into_iter()
        .skip(offset as usize)
        .take(normalized_page_size as usize)
        .collect();

    transaction.commit().map_err(sqlite_error)?;

    Ok(ProcessFeedPage {
        generated_at,
        page: effective_page,
        page_size: normalized_page_size,
        total_items,
        total_pages,
        has_previous_page: total_pages > 0 && effective_page > 1,
        has_next_page: total_pages > 0 && effective_page < total_pages,
        items,
    })
}

/// Lists persisted build runs enriched for operator-facing history surfaces.
pub fn list_build_history_records(
    storage: &StorageLayout,
) -> io::Result<Vec<BuildHistoryRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT br.id,
                   br.release_run_id,
                   rr.repository_id,
                   r.name,
                   r.repo_url,
                   rr.git_tag,
                   rr.git_commit,
                   br.build_target_id,
                   bt.name,
                   r.engine_kind,
                   COALESCE(bt.build_kind, ''),
                   COALESCE(bt.contract_json, ''),
                   bt.runner_type,
                   br.engine_version,
                   br.image_ref,
                   br.status,
                   br.workspace_path,
                   br.log_path,
                   br.artifact_root_path,
                   br.started_at,
                   br.finished_at,
                   br.error_message,
                   COUNT(DISTINCT a.id) AS artifact_count,
                   COUNT(DISTINCT pr.id) AS publish_run_count,
                   br.created_at,
                   br.updated_at
            FROM build_runs br
            JOIN release_runs rr ON rr.id = br.release_run_id
            JOIN repositories r ON r.id = rr.repository_id
            JOIN build_targets bt ON bt.id = br.build_target_id
            LEFT JOIN artifacts a ON a.build_run_id = br.id
            LEFT JOIN publish_runs pr ON pr.build_run_id = br.id
            GROUP BY br.id,
                     br.release_run_id,
                     rr.repository_id,
                     r.name,
                     r.repo_url,
                     rr.git_tag,
                     rr.git_commit,
                     br.build_target_id,
                     bt.name,
                     r.engine_kind,
                     COALESCE(bt.build_kind, ''),
                     COALESCE(bt.contract_json, ''),
                     bt.runner_type,
                     br.engine_version,
                     br.image_ref,
                     br.status,
                     br.workspace_path,
                     br.log_path,
                     br.artifact_root_path,
                     br.started_at,
                     br.finished_at,
                     br.error_message,
                     br.created_at,
                     br.updated_at
            ORDER BY COALESCE(
                         br.finished_at,
                         br.started_at,
                         br.updated_at,
                         br.created_at
                     ) DESC,
                     br.id DESC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], scan_build_history_record)
        .map_err(sqlite_error)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(sqlite_error)?);
    }

    Ok(records)
}

/// Lists persisted artifacts enriched for operator-facing inspection surfaces.
pub fn list_artifact_inspection_records(
    storage: &StorageLayout,
) -> io::Result<Vec<ArtifactInspectionRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT a.id,
                   a.build_run_id,
                   br.release_run_id,
                   rr.repository_id,
                   r.name,
                   r.repo_url,
                   rr.git_tag,
                   rr.git_commit,
                   br.build_target_id,
                   bt.name,
                   r.engine_kind,
                   COALESCE(bt.build_kind, ''),
                   COALESCE(bt.contract_json, ''),
                   bt.runner_type,
                   br.status,
                   a.name,
                   a.kind,
                   a.path,
                   br.artifact_root_path,
                   a.active_location_kind,
                   a.active_location_ref,
                   a.size_bytes,
                   a.checksum_sha256,
                   COUNT(DISTINCT pr.id) AS publish_run_count,
                   SUM(CASE WHEN pr.status = 'queued' THEN 1 ELSE 0 END)
                       AS queued_publish_runs,
                   SUM(CASE WHEN pr.status = 'running' THEN 1 ELSE 0 END)
                       AS running_publish_runs,
                   SUM(CASE WHEN pr.status = 'succeeded' THEN 1 ELSE 0 END)
                       AS succeeded_publish_runs,
                   SUM(CASE WHEN pr.status = 'failed' THEN 1 ELSE 0 END)
                       AS failed_publish_runs,
                   SUM(CASE WHEN pr.status = 'canceled' THEN 1 ELSE 0 END)
                       AS canceled_publish_runs,
                   a.created_at
            FROM artifacts a
            JOIN build_runs br ON br.id = a.build_run_id
            JOIN release_runs rr ON rr.id = br.release_run_id
            JOIN repositories r ON r.id = rr.repository_id
            JOIN build_targets bt ON bt.id = br.build_target_id
            LEFT JOIN publish_runs pr ON pr.artifact_id = a.id
            GROUP BY a.id,
                     a.build_run_id,
                     br.release_run_id,
                     rr.repository_id,
                     r.name,
                     r.repo_url,
                     rr.git_tag,
                     rr.git_commit,
                     br.build_target_id,
                     bt.name,
                     r.engine_kind,
                     COALESCE(bt.build_kind, ''),
                     COALESCE(bt.contract_json, ''),
                     bt.runner_type,
                     br.status,
                     a.name,
                     a.kind,
                     a.path,
                     br.artifact_root_path,
                     a.active_location_kind,
                     a.active_location_ref,
                     a.size_bytes,
                     a.checksum_sha256,
                     a.created_at
            ORDER BY COALESCE(br.finished_at, br.started_at, a.created_at) DESC,
                     a.id DESC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], scan_artifact_inspection_record)
        .map_err(sqlite_error)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(sqlite_error)?);
    }

    drop(statement);

    for record in &mut records {
        record.publish_runs =
            list_artifact_publish_runs_with_connection(&connection, record.artifact_id)?;
    }

    Ok(records)
}

fn list_artifact_publish_runs_with_connection(
    connection: &Connection,
    artifact_id: i64,
) -> io::Result<Vec<ArtifactPublishRunRecord>> {
    let mut statement = connection
        .prepare(
            "
            SELECT pr.id,
                   pr.publish_target_id,
                   pt.name,
                   pt.kind,
                   pr.status,
                   pr.destination_ref,
                   pr.created_at,
                   pr.updated_at
            FROM publish_runs pr
            JOIN publish_targets pt ON pt.id = pr.publish_target_id
            WHERE pr.artifact_id = ?
            ORDER BY pr.id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([artifact_id], |row| {
            Ok(ArtifactPublishRunRecord {
                publish_run_id: row.get(0)?,
                publish_target_id: row.get(1)?,
                publish_target_name: row.get::<_, String>(2)?.trim().to_owned(),
                publish_target_kind: row.get::<_, String>(3)?.trim().to_owned(),
                status: row.get::<_, String>(4)?.trim().to_owned(),
                destination_ref: normalize_optional_string(row.get(5)?),
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(sqlite_error)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(sqlite_error)?);
    }

    Ok(records)
}

/// Lists every persisted build target needed by shell settings and diagnostics.
pub fn list_build_target_runtime_settings(
    storage: &StorageLayout,
) -> io::Result<Vec<BuildTargetRuntimeSettingsRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT bt.id,
                   bt.repository_id,
                   r.name,
                   bt.name,
                     r.engine_kind,
                     COALESCE(bt.build_kind, ''),
                     COALESCE(bt.contract_json, ''),
                   bt.runner_type,
                   bt.enabled,
                   bt.config_json
            FROM build_targets bt
            JOIN repositories r ON r.id = bt.repository_id
            ORDER BY r.name ASC, bt.id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            let engine_kind: String = row.get(4)?;
            let build_kind: String = row.get(5)?;
            let contract_json: String = row.get(6)?;
            let projection = resolve_build_target_read_model_projection(
                engine_kind.trim(),
                build_kind.trim(),
                &contract_json,
            )
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

            Ok(BuildTargetRuntimeSettingsRecord {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                repository_name: row.get(2)?,
                name: row.get(3)?,
                unity_target_platform: projection.unity_target_platform,
                runner_type: row.get::<_, String>(7)?.trim().to_owned(),
                unity_build_method: projection.unity_build_method,
                enabled: row.get::<_, i64>(8)? != 0,
                config_json: row.get(9)?,
            })
        })
        .map_err(sqlite_error)?;

    let mut targets = Vec::new();
    for row in rows {
        targets.push(row.map_err(sqlite_error)?);
    }

    Ok(targets)
}

/// Lists every persisted credential row needed by shell settings and diagnostics.
pub fn list_credential_records(storage: &StorageLayout) -> io::Result<Vec<CredentialRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT id,
                   name,
                   kind,
                   config_json,
                   created_at,
                   updated_at
            FROM credentials
            ORDER BY name ASC, id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], scan_credential_record)
        .map_err(sqlite_error)?;

    let mut credentials = Vec::new();
    for row in rows {
        credentials.push(row.map_err(sqlite_error)?);
    }

    Ok(credentials)
}

/// Lists every persisted publish target needed by shell settings and diagnostics.
pub fn list_publish_target_runtime_settings(
    storage: &StorageLayout,
) -> io::Result<Vec<PublishTargetRuntimeSettingsRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT pt.id,
                   pt.repository_id,
                   r.name,
                   pt.name,
                   pt.kind,
                     pt.config_json,
                   pt.credentials_id,
                   pt.enabled
            FROM publish_targets pt
            JOIN repositories r ON r.id = pt.repository_id
            ORDER BY r.name ASC, pt.id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok(PublishTargetRuntimeSettingsRecord {
                id: row.get(0)?,
                repository_id: row.get(1)?,
                repository_name: row.get(2)?,
                name: row.get(3)?,
                kind: row.get::<_, String>(4)?.trim().to_owned(),
                config_json: row.get(5)?,
                credentials_id: row.get(6)?,
                enabled: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(sqlite_error)?;

    let mut targets = Vec::new();
    for row in rows {
        targets.push(row.map_err(sqlite_error)?);
    }

    Ok(targets)
}

/// Lists every persisted publish binding needed by shell inspection surfaces.
pub fn list_publish_target_binding_runtime_settings(
    storage: &StorageLayout,
) -> io::Result<Vec<PublishTargetBindingRuntimeSettingsRecord>> {
    let connection = open_connection(&storage.database_path)?;
    let mut statement = connection
        .prepare(
            "
            SELECT bpb.publish_target_id,
                   pt.repository_id,
                   bpb.build_target_id,
                   bt.name,
                   bpb.enabled,
                   bpb.options_json,
                   pt.kind
            FROM build_publish_bindings bpb
            JOIN publish_targets pt ON pt.id = bpb.publish_target_id
            JOIN repositories r ON r.id = pt.repository_id
            JOIN build_targets bt ON bt.id = bpb.build_target_id
            ORDER BY r.name ASC, bpb.publish_target_id ASC, bt.name ASC, bpb.id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(sqlite_error)?;

    let mut bindings = Vec::new();
    for row in rows {
        let (
            publish_target_id,
            repository_id,
            build_target_id,
            build_target_name,
            enabled,
            options_json,
            publish_target_kind,
        ) = row.map_err(sqlite_error)?;
        let consumption_behavior = String::from(
            classify_publish_binding_consumption(&publish_target_kind, &options_json)?.as_str(),
        );
        bindings.push(PublishTargetBindingRuntimeSettingsRecord {
            publish_target_id,
            repository_id,
            build_target_id,
            build_target_name,
            enabled,
            options_json,
            consumption_behavior,
        });
    }

    Ok(bindings)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessFeedReleaseRow {
    release: ReleaseRunRecord,
    repository_name: String,
    repository_url: Option<String>,
    repository_engine_kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RunStatusCounts {
    total: i64,
    queued: i64,
    running: i64,
    succeeded: i64,
    failed: i64,
    canceled: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessFeedBuildRunContext {
    target_name: String,
    unity_target_platform: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessFeedPublishRunContext {
    publish_target_name: String,
    artifact_name: Option<String>,
}

const PROCESS_FEED_ON_HOLD_STATUS: &str = "on_hold";
const PROCESS_FEED_LOCAL_WORKSPACE_HOLD_PREFIX: &str =
    "[hgp-on-hold-local-workspace-editor-open]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabasePragmas {
    busy_timeout_millis: u64,
    foreign_keys_enabled: bool,
    journal_mode: String,
}

fn apply_pragmas(connection: &Connection) -> io::Result<()> {
    for statement in [
        format!("PRAGMA busy_timeout = {SQLITE_BUSY_TIMEOUT_MILLIS};"),
        "PRAGMA foreign_keys = ON;".to_owned(),
        "PRAGMA journal_mode = WAL;".to_owned(),
    ] {
        connection.execute_batch(&statement).map_err(sqlite_error)?;
    }

    connection
        .busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MILLIS))
        .map_err(sqlite_error)?;

    Ok(())
}

fn ensure_migration_ledger(connection: &Connection) -> io::Result<()> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                name TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            ",
        )
        .map_err(sqlite_error)
}

fn apply_migrations(connection: &mut Connection) -> io::Result<Vec<String>> {
    let mut applied_migrations = Vec::new();

    for migration in MIGRATIONS {
        if migration_applied(connection, migration.name)? {
            continue;
        }

        let migration_sql = resolve_migration_sql(connection, migration)?;

        if migration.transactional {
            let transaction = connection.transaction().map_err(sqlite_error)?;
            transaction
                .execute_batch(migration_sql.as_ref())
                .map_err(sqlite_error)?;
            transaction
                .execute(
                    "
                    INSERT INTO schema_migrations (name, applied_at)
                    VALUES (?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                    ",
                    params![migration.name],
                )
                .map_err(sqlite_error)?;
            transaction.commit().map_err(sqlite_error)?;
        } else {
            if let Err(error) = connection.execute_batch(migration_sql.as_ref()) {
                let _ = connection.execute_batch("PRAGMA foreign_keys = ON;");
                return Err(sqlite_error(error));
            }
            connection
                .execute(
                    "
                    INSERT INTO schema_migrations (name, applied_at)
                    VALUES (?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                    ",
                    params![migration.name],
                )
                .map_err(sqlite_error)?;
        }

        applied_migrations.push(migration.name.to_owned());
    }

    Ok(applied_migrations)
}

fn migration_applied(connection: &Connection, name: &str) -> io::Result<bool> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(1) FROM schema_migrations WHERE name = ?",
            params![name],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;

    Ok(count > 0)
}

fn resolve_migration_sql(
    connection: &Connection,
    migration: &Migration,
) -> io::Result<Cow<'static, str>> {
    match migration.name {
        "0009_build_target_runner_model_cleanup.sql" => {
            if table_has_column(connection, "build_targets", "contract_json")? {
                Ok(Cow::Borrowed(MIGRATION_NO_OP_SQL))
            } else {
                Err(unsupported_pre_contract_build_target_schema(migration.name))
            }
        }
        "0010_engine_contract_model.sql" => {
            let build_targets_have_contract =
                table_has_column(connection, "build_targets", "contract_json")?;
            let repositories_have_engine_kind =
                table_has_column(connection, "repositories", "engine_kind")?;

            if !build_targets_have_contract {
                return Err(unsupported_pre_contract_build_target_schema(migration.name));
            }

            if repositories_have_engine_kind {
                Ok(Cow::Borrowed(MIGRATION_NO_OP_SQL))
            } else {
                Ok(Cow::Borrowed(migration.sql))
            }
        }
        "0011_runtime_engine_version.sql" => {
            let release_runs_have_engine_version =
                table_has_column(connection, "release_runs", "engine_version")?;
            let build_runs_have_engine_version =
                table_has_column(connection, "build_runs", "engine_version")?;
            let release_runs_have_unity_version =
                table_has_column(connection, "release_runs", "unity_version")?;
            let build_runs_have_unity_version =
                table_has_column(connection, "build_runs", "unity_version")?;

            let mut sql = String::new();
            if !release_runs_have_engine_version && release_runs_have_unity_version {
                sql.push_str(RENAME_RELEASE_RUN_ENGINE_VERSION_SQL);
            }
            if !build_runs_have_engine_version && build_runs_have_unity_version {
                sql.push_str(RENAME_BUILD_RUN_ENGINE_VERSION_SQL);
            }

            if sql.is_empty() {
                Ok(Cow::Borrowed(MIGRATION_NO_OP_SQL))
            } else {
                Ok(Cow::Owned(sql))
            }
        }
        _ => Ok(Cow::Borrowed(migration.sql)),
    }
}

fn unsupported_pre_contract_build_target_schema(migration_name: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "unsupported pre-contract build target schema during migration {migration_name}; reset the local runtime state before starting HGP"
        ),
    )
}

fn table_has_column(connection: &Connection, table_name: &str, column_name: &str) -> io::Result<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table_name})"))
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sqlite_error)?;

    for row in rows {
        if row.map_err(sqlite_error)? == column_name {
            return Ok(true);
        }
    }

    Ok(false)
}

fn read_pragmas(connection: &Connection) -> io::Result<DatabasePragmas> {
    let busy_timeout_millis = connection
        .query_row("PRAGMA busy_timeout;", [], |row| row.get::<_, u64>(0))
        .map_err(sqlite_error)?;
    let foreign_keys_enabled = connection
        .query_row("PRAGMA foreign_keys;", [], |row| row.get::<_, i64>(0))
        .map_err(sqlite_error)?
        == 1;
    let journal_mode = connection
        .query_row("PRAGMA journal_mode;", [], |row| row.get::<_, String>(0))
        .map_err(sqlite_error)?;

    Ok(DatabasePragmas {
        busy_timeout_millis,
        foreign_keys_enabled,
        journal_mode,
    })
}

fn invalid_input_error(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, message.into())
}

fn not_found_error(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::NotFound, message.into())
}

fn require_positive_identifier(value: i64, label: &str) -> io::Result<()> {
    if value <= 0 {
        return Err(invalid_input_error(format!(
            "{label} must be greater than zero"
        )));
    }

    Ok(())
}

fn build_dispatch_lock_key(build_run_id: i64) -> String {
    format!("build-run:{build_run_id}:dispatch")
}

fn publish_dispatch_lock_key(publish_run_id: i64) -> String {
    format!("publish-run:{publish_run_id}:dispatch")
}

fn release_planning_lock_key(release_run_id: i64) -> String {
    format!("release-plan:{release_run_id}")
}

fn build_dispatch_idempotency_key(build_run_id: i64, created_at: &str) -> String {
    dispatch_idempotency_key("build-run", build_run_id, created_at)
}

fn publish_dispatch_idempotency_key(publish_run_id: i64, created_at: &str) -> String {
    dispatch_idempotency_key("publish-run", publish_run_id, created_at)
}

fn dispatch_idempotency_key(prefix: &str, identifier: i64, created_at: &str) -> String {
    let created_at = created_at.trim();
    if created_at.is_empty() {
        return format!("{prefix}:{identifier}:queued");
    }

    let created_at_token = created_at.replace(' ', "T").replace(':', "-");
    format!("{prefix}:{identifier}:{created_at_token}:queued")
}

fn resolve_target_engine_version(
    target: &BuildTargetPlanningState,
    repository_engine_kind: EngineKind,
    release_engine_version: &str,
) -> io::Result<String> {
    let contract_engine_version = resolve_target_contract_engine_version(
        repository_engine_kind,
        target,
    )?;
    Ok(contract_engine_version
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(release_engine_version.trim())
        .to_owned())
}

fn resolve_target_contract_engine_version(
    repository_engine_kind: EngineKind,
    target: &BuildTargetPlanningState,
) -> io::Result<Option<String>> {
    match repository_engine_kind {
        EngineKind::Unity => resolve_unity_target_contract_engine_version(target),
        other => Err(invalid_input_error(format!(
            "repository engine_kind {:?} is not supported for build planning",
            other.as_str()
        ))),
    }
}

fn resolve_unity_target_contract_engine_version(
    target: &BuildTargetPlanningState,
) -> io::Result<Option<String>> {
    if target.build_kind != BuildKind::Player {
        return Err(invalid_input_error(format!(
            "build target {} uses unsupported build_kind {:?} for Unity planning",
            target.id,
            target.build_kind.as_str()
        )));
    }

    let contract_json = target.contract_json.trim();
    if contract_json.is_empty() {
        return Err(invalid_input_error(format!(
            "build target {} is missing contract_json for Unity planning",
            target.id
        )));
    }

    let contract = serde_json::from_str::<RepositoryProjectBuildContractInput>(contract_json)
        .map_err(|error| {
            invalid_input_error(format!(
                "build target {} has invalid contract_json for planning: {error}",
                target.id
            ))
        })?;
    let Some(unity) = contract.unity else {
        return Err(invalid_input_error(format!(
            "build target {} is missing contract_json.unity for Unity planning",
            target.id
        )));
    };

    Ok(normalize_optional_string(Some(unity.editor_version)))
}

fn resolve_build_image_ref(target: &BuildTargetPlanningState, engine_version: &str) -> io::Result<String> {
    let runner_type = target.runner_type.trim();
    if runner_type == DEFAULT_HOST_NATIVE_RUNNER_TYPE {
        return Ok(String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE));
    }

    let engine_version = engine_version.trim();
    if engine_version.is_empty() {
        return Err(invalid_input_error(
            "engine version is required for build planning".to_owned(),
        ));
    }

    Err(invalid_input_error(format!(
        "runner_type {runner_type:?} is not supported for Tauri runtime planning"
    )))
}

fn decode_release_dispatch_job(payload: &[u8]) -> io::Result<ReleaseDispatchJob> {
    let job: ReleaseDispatchJob = serde_json::from_slice(payload).map_err(|error| {
        io::Error::new(
            ErrorKind::InvalidData,
            format!("decode release job payload: {error}"),
        )
    })?;
    if job.release_run_id <= 0 {
        return Err(invalid_input_error(
            "release job release_run_id must be greater than zero",
        ));
    }
    if job.repository_id <= 0 {
        return Err(invalid_input_error(
            "release job repository_id must be greater than zero",
        ));
    }
    if job.git_tag.trim().is_empty() {
        return Err(invalid_input_error("release job git_tag must not be empty"));
    }
    if job.trigger_source.trim().is_empty() {
        return Err(invalid_input_error(
            "release job trigger_source must not be empty",
        ));
    }

    Ok(job)
}

fn build_runs_block_repository_queue(runs: &[BuildRunRecord]) -> bool {
    runs.iter().any(|run| {
        run.status == BuildStatus::Queued.as_str() || run.status == BuildStatus::Running.as_str()
    })
}

fn is_release_planning_noop_error(error: &io::Error) -> bool {
    if error.kind() != ErrorKind::InvalidInput {
        return false;
    }

    let message = error.to_string();
    message.contains("must be queued before build planning")
        || message.contains("has no enabled build targets")
}

fn detect_release_engine_version_from_source(
    repository_engine_kind: EngineKind,
    repository_url: &str,
    source: &ResolvedReleaseSourceMetadata,
    git_auth: &GitAuthOptions,
) -> io::Result<String> {
    match repository_engine_kind {
        EngineKind::Unity => detect_release_unity_engine_version_from_source(
            repository_url,
            source,
            git_auth,
        ),
        other => Err(invalid_input_error(format!(
            "repository engine_kind {:?} is not supported for release planning",
            other.as_str()
        ))),
    }
}

fn detect_release_unity_engine_version_from_source(
    repository_url: &str,
    source: &ResolvedReleaseSourceMetadata,
    git_auth: &GitAuthOptions,
) -> io::Result<String> {
    match source.source_kind.as_str() {
        RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => {
            detect_release_unity_engine_version_from_local_workspace(
                source.local_path.as_deref().ok_or_else(|| {
                    invalid_input_error("release local workspace path must not be empty")
                })?,
            )
        }
        RELEASE_SOURCE_KIND_MANAGED_TAG | RELEASE_SOURCE_KIND_MANAGED_REF => {
            let git_ref = source.source_ref.as_deref().ok_or_else(|| {
                invalid_input_error("release source_ref must not be empty")
            })?;
            detect_release_unity_engine_version_from_git_ref(
                repository_url,
                git_ref,
                git_auth,
            )
        }
        other => Err(invalid_input_error(format!(
            "unsupported release source_kind {:?}",
            other
        ))),
    }
}

fn detect_release_unity_engine_version_from_local_workspace(
    local_path: &str,
) -> io::Result<String> {
    let local_path = require_non_empty(local_path, "local workspace path")?;
    let contents = fs::read(Path::new(&local_path).join(PROJECT_VERSION_FILE_PATH)).map_err(
        |error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "read {PROJECT_VERSION_FILE_PATH} from local workspace {local_path:?}: {error}"
                ),
            )
        },
    )?;

    parse_project_version_unity_version(&contents)
}

fn detect_release_unity_engine_version_from_git_ref(
    repository_url: &str,
    git_ref: &str,
    git_auth: &GitAuthOptions,
) -> io::Result<String> {
    let contents = read_repository_file_from_git_ref(
        repository_url,
        git_ref,
        PROJECT_VERSION_FILE_PATH,
        git_auth,
    )?;
    parse_project_version_unity_version(&contents)
}

/// Reads the project version (`bundleVersion`) from a Unity local workspace.
///
/// Opens `ProjectSettings/ProjectSettings.asset` under `local_path` and returns
/// the first non-empty `bundleVersion:` value found. Returns an `InvalidData`
/// error if the file is unreadable or the field is absent.
pub fn read_unity_local_workspace_version(local_path: &str) -> io::Result<String> {
    detect_release_version_from_local_workspace(EngineKind::Unity, local_path)
}

fn detect_release_version_from_local_workspace(
    repository_engine_kind: EngineKind,
    local_path: &str,
) -> io::Result<String> {
    match repository_engine_kind {
        EngineKind::Unity => {
            let local_path = require_non_empty(local_path, "local workspace path")?;
            let contents = fs::read(Path::new(&local_path).join(PROJECT_SETTINGS_ASSET_PATH))
                .map_err(|error| {
                    io::Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "read {PROJECT_SETTINGS_ASSET_PATH} from local workspace {local_path:?}: {error}"
                        ),
                    )
                })?;
            parse_project_settings_bundle_version(&contents)
        }
        other => Err(invalid_input_error(format!(
            "repository engine_kind {:?} is not supported for on-demand version detection",
            other.as_str()
        ))),
    }
}

fn detect_release_version_from_git_ref(
    repository_engine_kind: EngineKind,
    repository_url: &str,
    git_ref: &str,
    git_auth: &GitAuthOptions,
) -> io::Result<String> {
    match repository_engine_kind {
        EngineKind::Unity => {
            let contents = read_repository_file_from_git_ref(
                repository_url,
                git_ref,
                PROJECT_SETTINGS_ASSET_PATH,
                git_auth,
            )?;
            parse_project_settings_bundle_version(&contents)
        }
        other => Err(invalid_input_error(format!(
            "repository engine_kind {:?} is not supported for on-demand version detection",
            other.as_str()
        ))),
    }
}

fn read_repository_file_from_git_ref(
    repository_url: &str,
    git_ref: &str,
    file_path: &str,
    git_auth: &GitAuthOptions,
) -> io::Result<Vec<u8>> {
    let repository_url = require_non_empty(repository_url, "repository url")?;
    let git_ref = require_non_empty(git_ref, "git ref")?;
    let file_path = require_non_empty(file_path, "repository file path")?;
    let workspace_path = std::env::temp_dir().join(next_token("release-source-workspace")?);
    let file_path_for_read = file_path.clone();

    let outcome = (|| {
        prepare_sparse_repository_file_workspace(
            &repository_url,
            &workspace_path,
            &git_ref,
            &file_path,
            git_auth,
        )?;
        fs::read(workspace_path.join(file_path_for_read.as_str())).map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "read {} from repository ref {:?}: {error}",
                    file_path,
                    git_ref,
                ),
            )
        })
    })();

    if workspace_path.exists() {
        let _ = fs::remove_dir_all(&workspace_path);
    }

    outcome
}

fn prepare_sparse_repository_file_workspace(
    repository_url: &str,
    workspace_path: &Path,
    git_ref: &str,
    file_path: &str,
    git_auth: &GitAuthOptions,
) -> io::Result<()> {
    let clone_destination = workspace_path.display().to_string();
    run_git_command(
        None,
        git_auth.append_git_args([
            "clone",
            "--filter=blob:none",
            "--sparse",
            "--depth=1",
            "--single-branch",
            "--branch",
            git_ref,
            "--no-checkout",
            repository_url,
            clone_destination.as_str(),
        ]),
    )
    .map_err(|error| {
        io::Error::new(
            ErrorKind::Other,
            format!("materialize repository ref {git_ref:?}: {error}"),
        )
    })?;

    run_git_command(
        Some(workspace_path),
        git_auth.append_git_args(["sparse-checkout", "set", "--no-cone", file_path]),
    )
    .map_err(|error| {
        io::Error::new(
            ErrorKind::Other,
            format!("configure sparse checkout for {file_path}: {error}"),
        )
    })?;

    run_git_command(
        Some(workspace_path),
        git_auth.append_git_args(["checkout", "--detach", "--force"]),
    )
    .map_err(|error| {
        io::Error::new(
            ErrorKind::Other,
            format!("checkout repository ref {git_ref:?}: {error}"),
        )
    })
}

fn run_git_command(working_dir: Option<&Path>, args: Vec<String>) -> io::Result<()> {
    let command_preview = args.join(" ");
    let mut command = git_command();
    command.args(args.iter().map(String::as_str));
    command.env("GIT_TERMINAL_PROMPT", "0");
    if let Some(working_dir) = working_dir {
        command.current_dir(working_dir);
    }

    let output = command.output().map_err(|error| {
        io::Error::new(
            ErrorKind::Other,
            format!("spawn git {command_preview}: {error}"),
        )
    })?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let details = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };

    Err(io::Error::new(
        ErrorKind::Other,
        format!("git {command_preview} failed: {details}"),
    ))
}

fn git_command() -> Command {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE") {
        return Command::new(path);
    }

    Command::new("git")
}

fn parse_project_version_unity_version(contents: &[u8]) -> io::Result<String> {
    for line in String::from_utf8_lossy(contents).lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("m_EditorVersion:") {
            continue;
        }

        let unity_version = trimmed.trim_start_matches("m_EditorVersion:").trim();
        if unity_version.is_empty() {
            break;
        }

        return Ok(unity_version.to_owned());
    }

    Err(io::Error::new(
        ErrorKind::InvalidData,
        format!(
            "{PROJECT_VERSION_FILE_PATH} does not define m_EditorVersion"
        ),
    ))
}

fn parse_project_settings_bundle_version(contents: &[u8]) -> io::Result<String> {
    for line in String::from_utf8_lossy(contents).lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("bundleVersion:") {
            continue;
        }

        let bundle_version = trimmed.trim_start_matches("bundleVersion:").trim();
        if bundle_version.is_empty() {
            break;
        }

        return Ok(bundle_version.to_owned());
    }

    Err(io::Error::new(
        ErrorKind::InvalidData,
        format!(
            "{PROJECT_SETTINGS_ASSET_PATH} does not define bundleVersion"
        ),
    ))
}

fn summarize_release_automation_status(
    release: &ReleaseRunRecord,
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> ReleaseAutomationStatus {
    let queued_build_runs = build_runs
        .iter()
        .filter(|run| run.status == BuildStatus::Queued.as_str())
        .count() as i64;
    let running_build_runs = build_runs
        .iter()
        .filter(|run| run.status == BuildStatus::Running.as_str())
        .count() as i64;
    let terminal_build_runs = build_runs
        .iter()
        .filter(|run| !build_run_is_active(&run.status))
        .count() as i64;
    let queued_publish_runs = publish_runs
        .iter()
        .filter(|run| run.status == PublishStatus::Queued.as_str())
        .count() as i64;
    let running_publish_runs = publish_runs
        .iter()
        .filter(|run| run.status == PublishStatus::Running.as_str())
        .count() as i64;
    let terminal_publish_runs = publish_runs
        .iter()
        .filter(|run| !publish_run_is_active(&run.status))
        .count() as i64;
    let build_process_active = build_runs.is_empty()
        || build_runs
            .iter()
            .any(|run| build_run_is_active(&run.status));
    let publish_process_active = publish_runs
        .iter()
        .any(|run| publish_run_is_active(&run.status));

    ReleaseAutomationStatus {
        release_run_id: release.id,
        git_tag: release.git_tag.clone(),
        engine_version: release.engine_version.clone(),
        status: release.status.clone(),
        planned: !build_runs.is_empty(),
        build_process_active,
        publish_process_active,
        queued_build_runs,
        running_build_runs,
        terminal_build_runs,
        total_build_runs: build_runs.len() as i64,
        queued_publish_runs,
        running_publish_runs,
        terminal_publish_runs,
        total_publish_runs: publish_runs.len() as i64,
    }
}

fn release_requires_automation_attention(
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> bool {
    build_runs.is_empty()
        || build_runs
            .iter()
            .any(|run| build_run_is_active(&run.status))
        || publish_runs
            .iter()
            .any(|run| publish_run_is_active(&run.status))
}

fn build_run_is_active(status: &str) -> bool {
    status == BuildStatus::Queued.as_str() || status == BuildStatus::Running.as_str()
}

fn publish_run_is_active(status: &str) -> bool {
    status == PublishStatus::Queued.as_str() || status == PublishStatus::Running.as_str()
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn normalize_manual_release_dispatch_input(
    input: ManualReleaseDispatchInput,
) -> io::Result<NormalizedManualReleaseDispatchInput> {
    if input.repository_id <= 0 {
        return Err(invalid_input_error(
            "repository id must be greater than zero",
        ));
    }

    let git_tag = require_non_empty(&input.git_tag, "git tag")?;
    let source_identity = build_release_source_identity(
        RELEASE_SOURCE_KIND_MANAGED_TAG,
        Some(&git_tag),
        None,
    )?;

    Ok(NormalizedManualReleaseDispatchInput {
        repository_id: input.repository_id,
        git_tag,
        git_commit: input.git_commit.trim().to_owned(),
        requested_via: input.requested_via.trim().to_owned(),
        source_identity,
    })
}

fn normalize_repository_poll_dispatch_input(
    input: RepositoryPollDispatchInput,
) -> io::Result<NormalizedRepositoryPollDispatchInput> {
    if input.repository_id <= 0 {
        return Err(invalid_input_error(
            "repository id must be greater than zero",
        ));
    }

    let git_tag = require_non_empty(&input.git_tag, "git tag")?;
    let source_identity = build_release_source_identity(
        RELEASE_SOURCE_KIND_MANAGED_TAG,
        Some(&git_tag),
        None,
    )?;

    Ok(NormalizedRepositoryPollDispatchInput {
        repository_id: input.repository_id,
        git_tag,
        git_commit: input.git_commit.trim().to_owned(),
        observed_via: input.observed_via.trim().to_owned(),
        source_identity,
    })
}

fn manual_dispatch_metadata_json(requested_via: &str, git_tag: &str) -> io::Result<String> {
    release_source_metadata_json(&ReleaseSourceMetadata {
        requested_via: normalize_optional_string(Some(requested_via.to_owned())),
        observed_via: None,
        source_kind: Some(String::from(RELEASE_SOURCE_KIND_MANAGED_TAG)),
        source_ref: Some(require_non_empty(git_tag, "git tag")?),
        local_path: None,
        version_source: Some(String::from(RELEASE_VERSION_SOURCE_MANUAL)),
        unity_executable_path_override: None,
    })
}

fn repository_poll_metadata_json(observed_via: &str, git_tag: &str) -> io::Result<String> {
    release_source_metadata_json(&ReleaseSourceMetadata {
        requested_via: None,
        observed_via: normalize_optional_string(Some(observed_via.to_owned())),
        source_kind: Some(String::from(RELEASE_SOURCE_KIND_MANAGED_TAG)),
        source_ref: Some(require_non_empty(git_tag, "git tag")?),
        local_path: None,
        version_source: None,
        unity_executable_path_override: None,
    })
}

fn on_demand_dispatch_metadata_json(
    input: &NormalizedOnDemandReleaseDispatchInput,
) -> io::Result<String> {
    release_source_metadata_json(&ReleaseSourceMetadata {
        requested_via: normalize_optional_string(Some(input.requested_via.clone())),
        observed_via: None,
        source_kind: Some(input.source_kind.clone()),
        source_ref: input.source_ref.clone(),
        local_path: input.local_path.clone(),
        version_source: Some(input.version_source.clone()),
        unity_executable_path_override: input.unity_executable_path_override.clone(),
    })
}

fn rebuild_release_metadata_json(
    release: &ReleaseRunRecord,
    requested_via: &str,
) -> io::Result<String> {
    let mut metadata = decode_release_source_metadata(
        &release.source_metadata_json,
        RELEASE_SOURCE_KIND_MANAGED_TAG,
        Some(&release.git_tag),
        None,
    )?;
    metadata.requested_via = normalize_optional_string(Some(requested_via.to_owned()));
    metadata.observed_via = None;
    release_source_metadata_json(&ReleaseSourceMetadata {
        requested_via: metadata.requested_via,
        observed_via: metadata.observed_via,
        source_kind: Some(metadata.source_kind),
        source_ref: metadata.source_ref,
        local_path: metadata.local_path,
        version_source: metadata.version_source,
        unity_executable_path_override: metadata.unity_executable_path_override,
    })
}

fn release_source_metadata_json(metadata: &ReleaseSourceMetadata) -> io::Result<String> {
    let mut object = serde_json::Map::new();

    if let Some(value) = metadata.requested_via.as_deref() {
        object.insert(String::from("requested_via"), serde_json::json!(value));
    }
    if let Some(value) = metadata.observed_via.as_deref() {
        object.insert(String::from("observed_via"), serde_json::json!(value));
    }
    if let Some(value) = metadata.source_kind.as_deref() {
        object.insert(String::from("source_kind"), serde_json::json!(value));
    }
    if let Some(value) = metadata.source_ref.as_deref() {
        object.insert(String::from("source_ref"), serde_json::json!(value));
    }
    if let Some(value) = metadata.local_path.as_deref() {
        object.insert(String::from("local_path"), serde_json::json!(value));
    }
    if let Some(value) = metadata.version_source.as_deref() {
        object.insert(String::from("version_source"), serde_json::json!(value));
    }
    if let Some(value) = metadata.unity_executable_path_override.as_deref() {
        object.insert(
            String::from("unity_executable_path_override"),
            serde_json::json!(value),
        );
    }

    serde_json::to_string(&serde_json::Value::Object(object))
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn decode_release_source_metadata(
    source_metadata_json: &str,
    default_source_kind: &str,
    default_source_ref: Option<&str>,
    default_local_path: Option<&str>,
) -> io::Result<ResolvedReleaseSourceMetadata> {
    let metadata = if source_metadata_json.trim().is_empty() {
        ReleaseSourceMetadata::default()
    } else {
        serde_json::from_str::<ReleaseSourceMetadata>(source_metadata_json).map_err(|error| {
            io::Error::new(
                ErrorKind::InvalidData,
                format!("decode release source metadata: {error}"),
            )
        })?
    };
    let source_kind = metadata
        .source_kind
        .as_deref()
        .map(normalize_release_source_kind)
        .transpose()?
        .unwrap_or_else(|| default_source_kind.to_owned());

    Ok(ResolvedReleaseSourceMetadata {
        requested_via: normalize_optional_string(metadata.requested_via),
        observed_via: normalize_optional_string(metadata.observed_via),
        source_kind,
        source_ref: normalize_optional_string(metadata.source_ref)
            .or_else(|| normalize_optional_string(default_source_ref.map(str::to_owned))),
        local_path: normalize_absolute_path_string(
            metadata.local_path.or_else(|| default_local_path.map(str::to_owned)),
            "release source metadata local_path",
        )?,
        version_source: metadata
            .version_source
            .as_deref()
            .map(normalize_release_version_source)
            .transpose()?,
        unity_executable_path_override: normalize_absolute_path_string(
            metadata.unity_executable_path_override,
            "release source metadata unity_executable_path_override",
        )?,
    })
}

fn normalize_release_source_kind(value: &str) -> io::Result<String> {
    match value.trim() {
        RELEASE_SOURCE_KIND_MANAGED_TAG => Ok(String::from(RELEASE_SOURCE_KIND_MANAGED_TAG)),
        RELEASE_SOURCE_KIND_MANAGED_REF => Ok(String::from(RELEASE_SOURCE_KIND_MANAGED_REF)),
        RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => {
            Ok(String::from(RELEASE_SOURCE_KIND_LOCAL_WORKSPACE))
        }
        other => Err(invalid_input_error(format!(
            "unsupported release source_kind {:?}",
            other
        ))),
    }
}

fn normalize_release_version_source(value: &str) -> io::Result<String> {
    match value.trim() {
        RELEASE_VERSION_SOURCE_MANUAL => Ok(String::from(RELEASE_VERSION_SOURCE_MANUAL)),
        RELEASE_VERSION_SOURCE_PROJECT_SETTINGS => {
            Ok(String::from(RELEASE_VERSION_SOURCE_PROJECT_SETTINGS))
        }
        other => Err(invalid_input_error(format!(
            "unsupported release version_source {:?}",
            other
        ))),
    }
}

fn normalize_absolute_path_string(
    value: Option<String>,
    label: &str,
) -> io::Result<Option<String>> {
    let Some(value) = normalize_optional_string(value) else {
        return Ok(None);
    };

    let normalized = PathBuf::from(&value).components().collect::<PathBuf>();
    if normalized.as_os_str().is_empty() || !normalized.is_absolute() {
        return Err(invalid_input_error(format!(
            "{label} must be an absolute path"
        )));
    }

    Ok(Some(normalized.to_string_lossy().replace('\\', "/")))
}

fn build_release_source_identity(
    source_kind: &str,
    source_ref: Option<&str>,
    local_path: Option<&str>,
) -> io::Result<String> {
    match source_kind {
        RELEASE_SOURCE_KIND_MANAGED_TAG | RELEASE_SOURCE_KIND_MANAGED_REF => Ok(format!(
            "{}:{}",
            source_kind,
            require_non_empty(source_ref.unwrap_or_default(), "release source ref")?
        )),
        RELEASE_SOURCE_KIND_LOCAL_WORKSPACE => {
            let mut normalized = require_non_empty(
                local_path.unwrap_or_default(),
                "release local workspace path",
            )?;
            if cfg!(windows) {
                normalized.make_ascii_lowercase();
            }
            Ok(format!("{}:{}", source_kind, normalized))
        }
        other => Err(invalid_input_error(format!(
            "unsupported release source_kind {:?}",
            other
        ))),
    }
}

fn release_dispatch_lock_key(release_run_id: i64) -> String {
    format!("release-run:{release_run_id}:dispatch")
}

fn release_dispatch_idempotency_key(release_run_id: i64) -> String {
    format!("release-run:{release_run_id}:queued")
}

fn nullable_string(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn list_process_feed_release_rows(
    transaction: &rusqlite::Transaction<'_>,
    scope: ProcessFeedScope,
    query: Option<&str>,
) -> io::Result<Vec<ProcessFeedReleaseRow>> {
    let scope_clause = match scope {
        ProcessFeedScope::All => None,
        ProcessFeedScope::Active => {
            Some("rr.status IN ('detected', 'queued', 'running')")
        }
    };
    let search_clause = query.map(|_| {
        "(CAST(rr.id AS TEXT) LIKE ?1
            OR rr.git_tag LIKE ?1
            OR COALESCE(rr.git_commit, '') LIKE ?1
            OR r.name LIKE ?1
            OR COALESCE(r.repo_url, '') LIKE ?1)"
    });
    let mut sql = String::from(
        "
            SELECT rr.id,
                   rr.repository_id,
                   r.name,
                   r.repo_url,
                   r.engine_kind,
                   rr.git_tag,
                   rr.git_commit,
                   rr.trigger_source,
                   rr.trigger_rule_id,
                   rr.source_metadata_json,
                   rr.source_identity,
                   rr.engine_version,
                   rr.status,
                   rr.started_at,
                   rr.finished_at,
                   rr.error_message,
                   rr.created_at,
                   rr.updated_at
            FROM release_runs rr
            JOIN repositories r ON r.id = rr.repository_id
        ",
    );

    if scope_clause.is_some() || search_clause.is_some() {
        sql.push_str(" WHERE ");
        if let Some(scope_clause) = scope_clause {
            sql.push_str(scope_clause);
        }
        if let Some(search_clause) = search_clause {
            if scope_clause.is_some() {
                sql.push_str(" AND ");
            }
            sql.push_str(search_clause);
        }
    }

    sql.push_str(
        "
            ORDER BY COALESCE(rr.started_at, rr.created_at) DESC,
                     rr.id DESC
        ",
    );

    let mut statement = transaction
        .prepare(&sql)
        .map_err(sqlite_error)?;
    let rows = if let Some(query) = query {
        let pattern = format!("%{query}%");
        statement
            .query_map(params![pattern], scan_process_feed_release_row)
            .map_err(sqlite_error)?
    } else {
        statement
            .query_map([], scan_process_feed_release_row)
            .map_err(sqlite_error)?
    };

    let mut releases = Vec::new();
    for row in rows {
        releases.push(row.map_err(sqlite_error)?);
    }

    Ok(releases)
}

fn normalize_process_feed_search_query(query: Option<&str>) -> Option<String> {
    let query = query?.trim();
    if query.is_empty() {
        None
    } else {
        Some(query.to_owned())
    }
}

fn process_feed_record_matches_status(
    record: &ProcessFeedRecord,
    status: ProcessFeedStatusFilter,
) -> bool {
    match status {
        ProcessFeedStatusFilter::All => true,
        ProcessFeedStatusFilter::Queued => record.display_status == "queued",
        ProcessFeedStatusFilter::Running => record.display_status == "running",
        ProcessFeedStatusFilter::OnHold => record.display_status == "on_hold",
        ProcessFeedStatusFilter::Succeeded => record.display_status == "succeeded",
        ProcessFeedStatusFilter::Failed => record.display_status == "failed",
        ProcessFeedStatusFilter::Canceled => record.display_status == "canceled",
    }
}

fn summarize_process_feed_record(
    row: ProcessFeedReleaseRow,
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
    build_run_contexts: &HashMap<i64, ProcessFeedBuildRunContext>,
    publish_run_contexts: &HashMap<i64, ProcessFeedPublishRunContext>,
) -> ProcessFeedRecord {
    let ProcessFeedReleaseRow {
        release,
        repository_name,
        repository_url,
        repository_engine_kind,
    } = row;
    let build_counts = count_run_statuses(build_runs.iter().map(|run| run.status.as_str()));
    let publish_counts =
        count_run_statuses(publish_runs.iter().map(|run| run.status.as_str()));
    let (current_step_label, current_step_status, current_step_detail) =
        summarize_process_feed_step(
            &release,
            build_runs,
            publish_runs,
            build_run_contexts,
            publish_run_contexts,
            build_counts,
            publish_counts,
        );
    let display_status = if current_step_status == PROCESS_FEED_ON_HOLD_STATUS {
        String::from(PROCESS_FEED_ON_HOLD_STATUS)
    } else {
        classify_process_feed_status(&release, build_counts, publish_counts).to_owned()
    };
    let error_message = select_process_feed_error_message(&release, build_runs, publish_runs);

    ProcessFeedRecord {
        release_run_id: release.id,
        repository_id: release.repository_id,
        repository_name,
        repository_url,
        repository_engine_kind,
        git_tag: release.git_tag,
        git_commit: release.git_commit,
        engine_version: release.engine_version,
        display_status,
        current_step_label,
        current_step_status,
        current_step_detail,
        queued_build_runs: build_counts.queued,
        running_build_runs: build_counts.running,
        succeeded_build_runs: build_counts.succeeded,
        failed_build_runs: build_counts.failed,
        canceled_build_runs: build_counts.canceled,
        queued_publish_runs: publish_counts.queued,
        running_publish_runs: publish_counts.running,
        succeeded_publish_runs: publish_counts.succeeded,
        failed_publish_runs: publish_counts.failed,
        canceled_publish_runs: publish_counts.canceled,
        total_build_runs: build_counts.total,
        total_publish_runs: publish_counts.total,
        started_at: release.started_at,
        finished_at: release.finished_at,
        error_message,
        created_at: release.created_at,
        updated_at: release.updated_at,
    }
}

fn summarize_process_feed_step(
    release: &ReleaseRunRecord,
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
    build_run_contexts: &HashMap<i64, ProcessFeedBuildRunContext>,
    publish_run_contexts: &HashMap<i64, ProcessFeedPublishRunContext>,
    build_counts: RunStatusCounts,
    publish_counts: RunStatusCounts,
) -> (String, String, Option<String>) {
    if let Some(run) = latest_build_run_by_status(build_runs, BuildStatus::Running.as_str()) {
        if let Some(hold_message) = process_feed_local_workspace_hold_message(run) {
            return (
                String::from("Process on hold"),
                String::from(PROCESS_FEED_ON_HOLD_STATUS),
                Some(hold_message),
            );
        }

        return (
            format_process_feed_running_build_label(
                run,
                build_run_contexts,
                build_counts.running,
            ),
            run.current_stage_status
                .clone()
                .unwrap_or_else(|| String::from(BuildStatus::Running.as_str())),
            run.last_progress_message.clone().or_else(|| {
                Some(format!(
                    "{} build target(s) are currently running",
                    build_counts.running
                ))
            }),
        );
    }

    if let Some(run) = latest_publish_run_by_status(publish_runs, PublishStatus::Running.as_str()) {
        return (
            format_process_feed_publish_label(
                "Publishing",
                publish_run_contexts,
                run.id,
                publish_counts.running,
            ),
            String::from(PublishStatus::Running.as_str()),
            Some(format!(
                "{} publish task(s) are currently running",
                publish_counts.running
            )),
        );
    }

    if let Some(run) = latest_build_run_by_status(build_runs, BuildStatus::Queued.as_str()) {
        return (
            format_process_feed_build_label(
                "Queued build:",
                build_run_contexts,
                run.id,
                build_counts.queued,
            ),
            String::from(BuildStatus::Queued.as_str()),
            Some(format!(
                "{} build target(s) are waiting to start",
                build_counts.queued
            )),
        );
    }

    if let Some(run) = latest_publish_run_by_status(publish_runs, PublishStatus::Queued.as_str()) {
        return (
            format_process_feed_publish_label(
                "Queued publish:",
                publish_run_contexts,
                run.id,
                publish_counts.queued,
            ),
            String::from(PublishStatus::Queued.as_str()),
            Some(format!(
                "{} publish task(s) are waiting to start",
                publish_counts.queued
            )),
        );
    }

    if let Some(run) = latest_build_run_by_status(build_runs, BuildStatus::Failed.as_str()) {
        return (
            format_process_feed_build_label(
                "Build failed:",
                build_run_contexts,
                run.id,
                build_counts.failed,
            ),
            String::from(BuildStatus::Failed.as_str()),
            run.error_message.clone().or(run.last_progress_message.clone()),
        );
    }

    if let Some(run) = latest_publish_run_by_status(publish_runs, PublishStatus::Failed.as_str()) {
        return (
            format_process_feed_publish_label(
                "Publish failed:",
                publish_run_contexts,
                run.id,
                publish_counts.failed,
            ),
            String::from(PublishStatus::Failed.as_str()),
            run.error_message.clone().or_else(|| {
                Some(format!(
                    "{} publish task(s) failed",
                    publish_counts.failed
                ))
            }),
        );
    }

    if let Some(run) = latest_build_run_by_status(build_runs, BuildStatus::Canceled.as_str()) {
        return (
            format_process_feed_build_label(
                "Build canceled:",
                build_run_contexts,
                run.id,
                build_counts.canceled,
            ),
            String::from(BuildStatus::Canceled.as_str()),
            run.error_message.clone().or(run.last_progress_message.clone()),
        );
    }

    if let Some(run) = latest_publish_run_by_status(publish_runs, PublishStatus::Canceled.as_str()) {
        return (
            format_process_feed_publish_label(
                "Publish canceled:",
                publish_run_contexts,
                run.id,
                publish_counts.canceled,
            ),
            String::from(PublishStatus::Canceled.as_str()),
            run.error_message.clone().or_else(|| {
                Some(format!(
                    "{} publish task(s) were canceled",
                    publish_counts.canceled
                ))
            }),
        );
    }

    if release.status == ReleaseStatus::Failed.as_str() {
        return (
            String::from("Release failed"),
            String::from(ReleaseStatus::Failed.as_str()),
            release.error_message.clone(),
        );
    }

    if release.status == ReleaseStatus::Canceled.as_str() {
        return (
            String::from("Release canceled"),
            String::from(ReleaseStatus::Canceled.as_str()),
            release.error_message.clone(),
        );
    }

    if release.status == ReleaseStatus::Succeeded.as_str()
        && build_counts.total == 0
        && publish_counts.total == 0
    {
        return (
            String::from("Completed"),
            String::from(ReleaseStatus::Succeeded.as_str()),
            None,
        );
    }

    if build_counts.total == 0 && publish_counts.total == 0 {
        return (
            String::from("Awaiting build planning"),
            if release.status == ReleaseStatus::Running.as_str() {
                String::from(ReleaseStatus::Running.as_str())
            } else {
                String::from(ReleaseStatus::Queued.as_str())
            },
            Some(String::from(
                "The runtime has not planned build targets for this process yet.",
            )),
        );
    }

    if publish_counts.total > 0 {
        return (
            latest_publish_run_by_status(publish_runs, PublishStatus::Succeeded.as_str())
                .map(|run| {
                    format_process_feed_publish_label(
                        "Published",
                        publish_run_contexts,
                        run.id,
                        publish_counts.succeeded,
                    )
                })
                .unwrap_or_else(|| {
                    format_process_feed_publish_count_label("Published", publish_counts.succeeded)
                }),
            String::from(PublishStatus::Succeeded.as_str()),
            Some(format!(
                "{} publish task(s) completed",
                publish_counts.succeeded
            )),
        );
    }

    if let Some(run) = latest_build_run_by_status(build_runs, BuildStatus::Succeeded.as_str()) {
        return (
            format_process_feed_build_label(
                "Built",
                build_run_contexts,
                run.id,
                build_counts.succeeded,
            ),
            String::from(BuildStatus::Succeeded.as_str()),
            Some(format!(
                "{} build target(s) completed",
                build_counts.succeeded
            )),
        );
    }

    (
        format_process_feed_build_count_label("Built", build_counts.succeeded),
        String::from(BuildStatus::Succeeded.as_str()),
        Some(format!(
            "{} build target(s) completed",
            build_counts.succeeded
        )),
    )
}

fn process_feed_local_workspace_hold_message(run: &BuildRunRecord) -> Option<String> {
    let message = run.last_progress_message.as_deref()?.trim();
    let hold_message = message.strip_prefix(PROCESS_FEED_LOCAL_WORKSPACE_HOLD_PREFIX)?;
    let normalized = hold_message.trim();
    if normalized.is_empty() {
        return None;
    }

    Some(normalized.to_owned())
}

fn format_process_feed_running_build_label(
    run: &BuildRunRecord,
    build_run_contexts: &HashMap<i64, ProcessFeedBuildRunContext>,
    running_count: i64,
) -> String {
    let prefix = resolve_process_feed_build_stage_prefix(run.current_stage_key.as_deref())
        .unwrap_or("Building");
    format_process_feed_build_label(prefix, build_run_contexts, run.id, running_count)
}

fn format_process_feed_build_label(
    prefix: &str,
    build_run_contexts: &HashMap<i64, ProcessFeedBuildRunContext>,
    build_run_id: i64,
    count: i64,
) -> String {
    let subject = resolve_process_feed_build_subject(build_run_contexts, build_run_id);
    format_process_feed_subject_label(prefix, subject.as_deref(), count, "target", "targets")
}

fn format_process_feed_publish_label(
    prefix: &str,
    publish_run_contexts: &HashMap<i64, ProcessFeedPublishRunContext>,
    publish_run_id: i64,
    count: i64,
) -> String {
    let subject = resolve_process_feed_publish_subject(publish_run_contexts, publish_run_id);
    format_process_feed_subject_label(
        prefix,
        subject.as_deref(),
        count,
        "destination",
        "destinations",
    )
}

fn format_process_feed_build_count_label(prefix: &str, count: i64) -> String {
    format_process_feed_count_label(prefix, count, "target", "targets")
}

fn format_process_feed_publish_count_label(prefix: &str, count: i64) -> String {
    format_process_feed_count_label(prefix, count, "destination", "destinations")
}

fn format_process_feed_subject_label(
    prefix: &str,
    subject: Option<&str>,
    count: i64,
    singular: &str,
    plural: &str,
) -> String {
    if let Some(subject) = subject.map(str::trim).filter(|subject| !subject.is_empty()) {
        if count > 1 {
            return format!("{prefix} {subject} +{} more", count - 1);
        }

        return format!("{prefix} {subject}");
    }

    format_process_feed_count_label(prefix, count, singular, plural)
}

fn format_process_feed_count_label(
    prefix: &str,
    count: i64,
    singular: &str,
    plural: &str,
) -> String {
    if count <= 1 {
        return format!("{prefix} {singular}");
    }

    format!("{prefix} {count} {plural}")
}

fn resolve_process_feed_build_stage_prefix(stage_key: Option<&str>) -> Option<&'static str> {
    let stage_key = stage_key?.trim().to_ascii_lowercase();

    if stage_key.contains("validate") {
        return Some("Preparing");
    }

    if stage_key.contains("package") {
        return Some("Packaging");
    }

    if stage_key.contains("register") {
        return Some("Registering");
    }

    if stage_key.contains("build") {
        return Some("Building");
    }

    None
}

fn resolve_process_feed_build_subject(
    build_run_contexts: &HashMap<i64, ProcessFeedBuildRunContext>,
    build_run_id: i64,
) -> Option<String> {
    let context = build_run_contexts.get(&build_run_id)?;
    let target_name = context.target_name.trim();
    let platform = humanize_unity_target_platform(&context.unity_target_platform);
    if !target_name.is_empty() {
        let alias = format_process_feed_build_target_alias(target_name, platform.as_str());
        if !alias.is_empty() {
            return Some(alias);
        }
    }

    if platform.is_empty() {
        None
    } else {
        Some(platform)
    }
}

fn format_process_feed_build_target_alias(target_name: &str, platform: &str) -> String {
    let normalized_target_name = target_name.trim();
    if normalized_target_name.is_empty() {
        return platform.to_owned();
    }

    if platform.is_empty() {
        return truncate_process_feed_label(normalized_target_name, 26);
    }

    let target_tokens = tokenize_process_feed_label(normalized_target_name);
    if target_tokens.is_empty() {
        return platform.to_owned();
    }

    let mentions_platform = target_tokens
        .iter()
        .any(|token| token_matches_unity_target_platform(token, platform));

    if mentions_platform {
        if target_tokens.len() >= 3 {
            return platform.to_owned();
        }

        if target_tokens.len() == 2
            && target_tokens
                .iter()
                .any(|token| is_generic_process_feed_build_token(token))
        {
            return platform.to_owned();
        }

        return truncate_process_feed_label(normalized_target_name, 26);
    }

    if normalized_target_name.chars().count() > 26 {
        return format!(
            "{} ({platform})",
            truncate_process_feed_label(normalized_target_name, 18)
        );
    }

    format!("{normalized_target_name} ({platform})")
}

fn tokenize_process_feed_label(value: &str) -> Vec<String> {
    value
        .split(|character: char| {
            matches!(character, '-' | '_' | '/' | '\\' | ' ' | '.' | ':')
        })
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn token_matches_unity_target_platform(token: &str, platform: &str) -> bool {
    let normalized_token = token.trim().to_ascii_lowercase();

    match platform.trim().to_ascii_lowercase().as_str() {
        "windows" => matches!(
            normalized_token.as_str(),
            "windows" | "win" | "win64" | "standalonewindows" | "standalonewindows64"
        ),
        "linux" => matches!(
            normalized_token.as_str(),
            "linux" | "linux64" | "standalonelinux" | "standalonelinux64"
        ),
        "macos" => matches!(
            normalized_token.as_str(),
            "mac" | "macos" | "osx" | "standaloneosx"
        ),
        "webgl" => matches!(normalized_token.as_str(), "webgl"),
        "android" => matches!(normalized_token.as_str(), "android"),
        other => normalized_token == other,
    }
}

fn is_generic_process_feed_build_token(token: &str) -> bool {
    matches!(
        token,
        "build"
            | "builds"
            | "player"
            | "players"
            | "target"
            | "targets"
            | "release"
            | "runtime"
            | "standalone"
            | "artifact"
            | "artifacts"
            | "output"
            | "outputs"
    )
}

fn truncate_process_feed_label(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_owned();
    }

    let truncated = trimmed.chars().take(max_chars.saturating_sub(3)).collect::<String>();
    format!("{truncated}...")
}

fn resolve_process_feed_publish_subject(
    publish_run_contexts: &HashMap<i64, ProcessFeedPublishRunContext>,
    publish_run_id: i64,
) -> Option<String> {
    let context = publish_run_contexts.get(&publish_run_id)?;
    let publish_target_name = context.publish_target_name.trim();
    if !publish_target_name.is_empty() {
        return Some(publish_target_name.to_owned());
    }

    context.artifact_name.clone()
}

fn humanize_unity_target_platform(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "windows" | "standalonewindows" | "standalonewindows64" => {
            String::from("Windows")
        }
        "linux" | "standalonelinux64" => String::from("Linux"),
        "mac" | "macos" | "osx" | "standaloneosx" => String::from("macOS"),
        "webgl" => String::from("WebGL"),
        "android" => String::from("Android"),
        _ => value.trim().to_owned(),
    }
}

fn classify_process_feed_status(
    release: &ReleaseRunRecord,
    build_counts: RunStatusCounts,
    publish_counts: RunStatusCounts,
) -> &'static str {
    if build_counts.running > 0
        || publish_counts.running > 0
        || release.status == ReleaseStatus::Running.as_str()
    {
        return "running";
    }

    if build_counts.queued > 0
        || publish_counts.queued > 0
        || release.status == ReleaseStatus::Detected.as_str()
        || release.status == ReleaseStatus::Queued.as_str()
    {
        return "queued";
    }

    if build_counts.failed > 0
        || publish_counts.failed > 0
        || release.status == ReleaseStatus::Failed.as_str()
    {
        return "failed";
    }

    if build_counts.canceled > 0
        || publish_counts.canceled > 0
        || release.status == ReleaseStatus::Canceled.as_str()
    {
        return "canceled";
    }

    "succeeded"
}

fn derive_release_run_reconciliation(
    release: &ReleaseRunRecord,
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> Option<ReleaseRunReconciliation> {
    if build_runs.is_empty() && publish_runs.is_empty() {
        return None;
    }

    let has_running = build_runs
        .iter()
        .any(|run| run.status == BuildStatus::Running.as_str())
        || publish_runs
            .iter()
            .any(|run| run.status == PublishStatus::Running.as_str());
    let has_queued = build_runs
        .iter()
        .any(|run| run.status == BuildStatus::Queued.as_str())
        || publish_runs
            .iter()
            .any(|run| run.status == PublishStatus::Queued.as_str());
    let has_failed = build_runs
        .iter()
        .any(|run| run.status == BuildStatus::Failed.as_str())
        || publish_runs
            .iter()
            .any(|run| run.status == PublishStatus::Failed.as_str());
    let has_canceled = build_runs
        .iter()
        .any(|run| run.status == BuildStatus::Canceled.as_str())
        || publish_runs
            .iter()
            .any(|run| run.status == PublishStatus::Canceled.as_str());
    let all_children_succeeded = build_runs
        .iter()
        .all(|run| run.status == BuildStatus::Succeeded.as_str())
        && publish_runs
            .iter()
            .all(|run| run.status == PublishStatus::Succeeded.as_str());

    let status = if has_running {
        ReleaseStatus::Running.as_str()
    } else if has_queued {
        ReleaseStatus::Queued.as_str()
    } else if has_failed {
        ReleaseStatus::Failed.as_str()
    } else if has_canceled {
        ReleaseStatus::Canceled.as_str()
    } else if all_children_succeeded {
        ReleaseStatus::Succeeded.as_str()
    } else {
        return None;
    };

    let started_at = release
        .started_at
        .clone()
        .or_else(|| first_release_child_started_at(build_runs, publish_runs));
    let finished_at = if status == ReleaseStatus::Succeeded.as_str()
        || status == ReleaseStatus::Failed.as_str()
        || status == ReleaseStatus::Canceled.as_str()
    {
        release
            .finished_at
            .clone()
            .or_else(|| latest_release_child_finished_at(build_runs, publish_runs))
    } else {
        None
    };
    let error_message = match status {
        value if value == ReleaseStatus::Failed.as_str() => latest_release_child_error_message(
            build_runs,
            publish_runs,
            BuildStatus::Failed.as_str(),
            PublishStatus::Failed.as_str(),
        )
        .or_else(|| release.error_message.clone()),
        value if value == ReleaseStatus::Canceled.as_str() => {
            latest_release_child_error_message(
                build_runs,
                publish_runs,
                BuildStatus::Canceled.as_str(),
                PublishStatus::Canceled.as_str(),
            )
            .or_else(|| release.error_message.clone())
        }
        _ => None,
    };

    Some(ReleaseRunReconciliation {
        status: String::from(status),
        started_at,
        finished_at,
        error_message,
    })
}

fn first_release_child_started_at(
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> Option<String> {
    build_runs
        .iter()
        .filter_map(|run| run.started_at.as_deref())
        .chain(
            publish_runs
                .iter()
                .filter_map(|run| run.started_at.as_deref()),
        )
        .min()
        .map(str::to_owned)
}

fn latest_release_child_finished_at(
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> Option<String> {
    build_runs
        .iter()
        .filter_map(|run| run.finished_at.as_deref())
        .chain(
            publish_runs
                .iter()
                .filter_map(|run| run.finished_at.as_deref()),
        )
        .max()
        .map(str::to_owned)
}

fn latest_release_child_error_message(
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
    build_status: &str,
    publish_status: &str,
) -> Option<String> {
    let build_candidate = latest_build_run_by_status(build_runs, build_status).and_then(|run| {
        run.error_message
            .as_ref()
            .map(|message| (build_run_activity_key(run), message.clone()))
    });
    let publish_candidate = latest_publish_run_by_status(publish_runs, publish_status)
        .and_then(|run| {
            run.error_message
                .as_ref()
                .map(|message| (publish_run_activity_key(run), message.clone()))
        });

    match (build_candidate, publish_candidate) {
        (Some((build_key, build_message)), Some((publish_key, publish_message))) => {
            if publish_key > build_key {
                Some(publish_message)
            } else {
                Some(build_message)
            }
        }
        (Some((_, message)), None) | (None, Some((_, message))) => Some(message),
        (None, None) => None,
    }
}

fn select_process_feed_error_message(
    release: &ReleaseRunRecord,
    build_runs: &[BuildRunRecord],
    publish_runs: &[PublishRunRecord],
) -> Option<String> {
    release
        .error_message
        .clone()
        .or_else(|| {
            latest_build_run_by_status(build_runs, BuildStatus::Failed.as_str())
                .and_then(|run| run.error_message.clone())
        })
        .or_else(|| {
            latest_publish_run_by_status(publish_runs, PublishStatus::Failed.as_str())
                .and_then(|run| run.error_message.clone())
        })
        .or_else(|| {
            latest_build_run_by_status(build_runs, BuildStatus::Canceled.as_str())
                .and_then(|run| run.error_message.clone())
        })
        .or_else(|| {
            latest_publish_run_by_status(publish_runs, PublishStatus::Canceled.as_str())
                .and_then(|run| run.error_message.clone())
        })
}

fn count_run_statuses<'a>(statuses: impl Iterator<Item = &'a str>) -> RunStatusCounts {
    let mut counts = RunStatusCounts::default();

    for status in statuses {
        counts.total += 1;

        match status {
            value if value == BuildStatus::Queued.as_str() => counts.queued += 1,
            value if value == BuildStatus::Running.as_str() => counts.running += 1,
            value if value == BuildStatus::Succeeded.as_str() => counts.succeeded += 1,
            value if value == BuildStatus::Failed.as_str() => counts.failed += 1,
            value if value == BuildStatus::Canceled.as_str() => counts.canceled += 1,
            _ => {}
        }
    }

    counts
}

fn latest_build_run_by_status<'a>(
    runs: &'a [BuildRunRecord],
    status: &str,
) -> Option<&'a BuildRunRecord> {
    runs.iter()
        .filter(|run| run.status == status)
        .max_by_key(|run| (build_run_activity_key(run), run.id))
}

fn latest_publish_run_by_status<'a>(
    runs: &'a [PublishRunRecord],
    status: &str,
) -> Option<&'a PublishRunRecord> {
    runs.iter()
        .filter(|run| run.status == status)
        .max_by_key(|run| (publish_run_activity_key(run), run.id))
}

fn build_run_activity_key(run: &BuildRunRecord) -> &str {
    run.heartbeat_at
        .as_deref()
        .or(run.finished_at.as_deref())
        .or(run.started_at.as_deref())
        .unwrap_or(run.updated_at.as_str())
}

fn publish_run_activity_key(run: &PublishRunRecord) -> &str {
    run.finished_at
        .as_deref()
        .or(run.started_at.as_deref())
        .unwrap_or(run.updated_at.as_str())
}

fn map_release_store_sqlite_error(error: rusqlite::Error) -> io::Error {
    let lower_error = error.to_string().to_ascii_lowercase();
    if lower_error.contains("unique constraint failed") {
        return io::Error::new(
            ErrorKind::AlreadyExists,
            format!("release run conflict: {error}"),
        );
    }
    if lower_error.contains("foreign key constraint failed") {
        return io::Error::new(
            ErrorKind::NotFound,
            format!("release repository not found: {error}"),
        );
    }

    sqlite_error(error)
}

fn scan_release_run_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReleaseRunRecord> {
    Ok(ReleaseRunRecord {
        id: row.get(0)?,
        repository_id: row.get(1)?,
        git_tag: row.get::<_, String>(2)?.trim().to_owned(),
        git_commit: normalize_optional_string(row.get(3)?),
        trigger_source: row.get::<_, String>(4)?.trim().to_owned(),
        trigger_rule_id: row.get(5)?,
        source_metadata_json: row.get(6)?,
        source_identity: row.get::<_, String>(7)?.trim().to_owned(),
        engine_version: normalize_optional_string(row.get(8)?),
        status: row.get(9)?,
        started_at: normalize_optional_string(row.get(10)?),
        finished_at: normalize_optional_string(row.get(11)?),
        error_message: normalize_optional_string(row.get(12)?),
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn scan_process_feed_release_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ProcessFeedReleaseRow> {
    Ok(ProcessFeedReleaseRow {
        release: ReleaseRunRecord {
            id: row.get(0)?,
            repository_id: row.get(1)?,
            git_tag: row.get::<_, String>(5)?.trim().to_owned(),
            git_commit: normalize_optional_string(row.get(6)?),
            trigger_source: row.get::<_, String>(7)?.trim().to_owned(),
            trigger_rule_id: row.get(8)?,
            source_metadata_json: row.get(9)?,
            source_identity: row.get::<_, String>(10)?.trim().to_owned(),
            engine_version: normalize_optional_string(row.get(11)?),
            status: row.get(12)?,
            started_at: normalize_optional_string(row.get(13)?),
            finished_at: normalize_optional_string(row.get(14)?),
            error_message: normalize_optional_string(row.get(15)?),
            created_at: row.get(16)?,
            updated_at: row.get(17)?,
        },
        repository_name: row.get(2)?,
        repository_url: normalize_optional_string(row.get(3)?),
        repository_engine_kind: row.get::<_, String>(4)?.trim().to_owned(),
    })
}

fn scan_build_run_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<BuildRunRecord> {
    Ok(BuildRunRecord {
        id: row.get(0)?,
        release_run_id: row.get(1)?,
        build_target_id: row.get(2)?,
        engine_version: normalize_optional_string(row.get(3)?),
        image_ref: normalize_optional_string(row.get(4)?),
        status: row.get(5)?,
        workspace_path: normalize_optional_string(row.get(6)?),
        log_path: normalize_optional_string(row.get(7)?),
        artifact_root_path: normalize_optional_string(row.get(8)?),
        current_stage_key: normalize_optional_string(row.get(9)?),
        current_stage_label: normalize_optional_string(row.get(10)?),
        current_stage_status: normalize_optional_string(row.get(11)?),
        heartbeat_at: normalize_optional_string(row.get(12)?),
        last_progress_message: normalize_optional_string(row.get(13)?),
        started_at: normalize_optional_string(row.get(14)?),
        finished_at: normalize_optional_string(row.get(15)?),
        error_message: normalize_optional_string(row.get(16)?),
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

fn scan_build_run_stage_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<BuildRunStageRecord> {
    Ok(BuildRunStageRecord {
        id: row.get(0)?,
        build_run_id: row.get(1)?,
        position: row.get(2)?,
        step_key: row.get(3)?,
        step_label: row.get(4)?,
        status: row.get(5)?,
        log_path: row.get(6)?,
        last_message: normalize_optional_string(row.get(7)?),
        heartbeat_at: normalize_optional_string(row.get(8)?),
        started_at: normalize_optional_string(row.get(9)?),
        finished_at: normalize_optional_string(row.get(10)?),
        error_message: normalize_optional_string(row.get(11)?),
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn scan_build_history_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<BuildHistoryRecord> {
    let engine_kind: String = row.get(9)?;
    let build_kind: String = row.get(10)?;
    let contract_json: String = row.get(11)?;
    let projection = resolve_build_target_read_model_projection(
        engine_kind.trim(),
        build_kind.trim(),
        &contract_json,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            11,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;

    Ok(BuildHistoryRecord {
        build_run_id: row.get(0)?,
        release_run_id: row.get(1)?,
        repository_id: row.get(2)?,
        repository_name: row.get(3)?,
        repository_url: normalize_optional_string(row.get(4)?),
        git_tag: row.get::<_, String>(5)?.trim().to_owned(),
        git_commit: normalize_optional_string(row.get(6)?),
        build_target_id: row.get(7)?,
        build_target_name: row.get(8)?,
        unity_target_platform: projection.unity_target_platform,
        runner_type: row.get::<_, String>(12)?.trim().to_owned(),
        unity_build_method: projection.unity_build_method,
        engine_version: normalize_optional_string(row.get(13)?),
        image_ref: normalize_optional_string(row.get(14)?),
        status: row.get(15)?,
        workspace_path: normalize_optional_string(row.get(16)?),
        log_path: normalize_optional_string(row.get(17)?),
        artifact_root_path: normalize_optional_string(row.get(18)?),
        started_at: normalize_optional_string(row.get(19)?),
        finished_at: normalize_optional_string(row.get(20)?),
        error_message: normalize_optional_string(row.get(21)?),
        artifact_count: row.get(22)?,
        publish_run_count: row.get(23)?,
        created_at: row.get(24)?,
        updated_at: row.get(25)?,
    })
}

fn scan_artifact_inspection_record(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ArtifactInspectionRecord> {
    let engine_kind: String = row.get(10)?;
    let build_kind: String = row.get(11)?;
    let contract_json: String = row.get(12)?;
    let projection = resolve_build_target_read_model_projection(
        engine_kind.trim(),
        build_kind.trim(),
        &contract_json,
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            12,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;

    Ok(ArtifactInspectionRecord {
        artifact_id: row.get(0)?,
        build_run_id: row.get(1)?,
        release_run_id: row.get(2)?,
        repository_id: row.get(3)?,
        repository_name: row.get::<_, String>(4)?.trim().to_owned(),
        repository_url: normalize_optional_string(row.get(5)?),
        git_tag: row.get::<_, String>(6)?.trim().to_owned(),
        git_commit: normalize_optional_string(row.get(7)?),
        build_target_id: row.get(8)?,
        build_target_name: row.get::<_, String>(9)?.trim().to_owned(),
        unity_target_platform: projection.unity_target_platform,
        runner_type: row.get::<_, String>(13)?.trim().to_owned(),
        build_status: row.get::<_, String>(14)?.trim().to_owned(),
        artifact_name: row.get::<_, String>(15)?.trim().to_owned(),
        artifact_kind: row.get::<_, String>(16)?.trim().to_owned(),
        artifact_path: row.get::<_, String>(17)?.trim().to_owned(),
        artifact_root_path: normalize_optional_string(row.get(18)?),
        artifact_active_location_kind: normalize_optional_string(row.get(19)?)
            .unwrap_or_else(|| String::from("runtime_artifact")),
        artifact_active_location_ref: normalize_optional_string(row.get(20)?)
            .unwrap_or_else(|| row.get::<_, String>(17).unwrap_or_default().trim().to_owned()),
        size_bytes: row.get(21)?,
        checksum_sha256: normalize_optional_string(row.get(22)?),
        publish_run_count: row.get(23)?,
        queued_publish_runs: row.get(24)?,
        running_publish_runs: row.get(25)?,
        succeeded_publish_runs: row.get(26)?,
        failed_publish_runs: row.get(27)?,
        canceled_publish_runs: row.get(28)?,
        publish_runs: Vec::new(),
        created_at: row.get(29)?,
    })
}

struct BuildPublishSummary {
    release_run_id: i64,
    build_target_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PublishBindingConsumption {
    NonConsuming,
    Consuming,
}

impl PublishBindingConsumption {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NonConsuming => "non_consuming",
            Self::Consuming => "consuming",
        }
    }

    const fn dispatch_rank(self) -> i64 {
        match self {
            Self::NonConsuming => 0,
            Self::Consuming => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishBindingPlanningRecord {
    binding_id: i64,
    publish_target_id: i64,
    publish_target_name: String,
    publish_target_kind: String,
    publish_target_config_json: String,
    publish_target_credentials_id: Option<i64>,
    binding_options_json: String,
    consumption: PublishBindingConsumption,
}

fn load_build_publish_summary(
    connection: &Connection,
    build_run_id: i64,
) -> io::Result<Option<BuildPublishSummary>> {
    connection
        .query_row(
            "
            SELECT release_run_id,
                   build_target_id
            FROM build_runs
            WHERE id = ?
            ",
            [build_run_id],
            |row| {
                Ok(BuildPublishSummary {
                    release_run_id: row.get(0)?,
                    build_target_id: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(sqlite_error)
}

fn list_build_artifacts_with_connection(
    connection: &Connection,
    build_run_id: i64,
) -> io::Result<Vec<ArtifactRecord>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id,
                   build_run_id,
                   name,
                   kind,
                   path,
                     active_location_kind,
                     active_location_ref,
                   size_bytes,
                   checksum_sha256,
                   created_at
            FROM artifacts
            WHERE build_run_id = ?
            ORDER BY id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_run_id], scan_artifact_record)
        .map_err(sqlite_error)?;

    let mut artifacts = Vec::new();
    for row in rows {
        artifacts.push(row.map_err(sqlite_error)?);
    }

    Ok(artifacts)
}

fn list_build_artifact_ids(connection: &Connection, build_run_id: i64) -> io::Result<Vec<i64>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id
            FROM artifacts
            WHERE build_run_id = ?
            ORDER BY id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_run_id], |row| row.get(0))
        .map_err(sqlite_error)?;

    let mut artifact_ids = Vec::new();
    for row in rows {
        artifact_ids.push(row.map_err(sqlite_error)?);
    }

    Ok(artifact_ids)
}

fn list_enabled_publish_binding_plans(
    connection: &Connection,
    build_target_id: i64,
) -> io::Result<Vec<PublishBindingPlanningRecord>> {
    let mut statement = connection
        .prepare(
            "
            SELECT bpb.id,
                   bpb.publish_target_id,
                   pt.name,
                   pt.kind,
                   pt.config_json,
                     pt.credentials_id,
                   bpb.options_json
            FROM build_publish_bindings bpb
            JOIN publish_targets pt ON pt.id = bpb.publish_target_id
            WHERE bpb.build_target_id = ?
              AND bpb.enabled = 1
            ORDER BY bpb.id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_target_id], |row| {
            let publish_target_kind = row.get::<_, String>(3)?.trim().to_owned();
            let binding_options_json = row.get::<_, String>(6)?;

            Ok(PublishBindingPlanningRecord {
                binding_id: row.get(0)?,
                publish_target_id: row.get(1)?,
                publish_target_name: row.get::<_, String>(2)?.trim().to_owned(),
                consumption: classify_publish_binding_consumption(
                    publish_target_kind.as_str(),
                    binding_options_json.as_str(),
                )
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                publish_target_kind,
                publish_target_config_json: row.get(4)?,
                publish_target_credentials_id: row.get(5)?,
                binding_options_json,
            })
        })
        .map_err(sqlite_error)?;

    let mut publish_bindings = Vec::new();
    for row in rows {
        publish_bindings.push(row.map_err(sqlite_error)?);
    }

    let consuming_count = publish_bindings
        .iter()
        .filter(|binding| binding.consumption == PublishBindingConsumption::Consuming)
        .count();
    if consuming_count > 1 {
        return Err(invalid_input_error(format!(
            "build target {build_target_id} cannot enable more than one consuming publish binding"
        )));
    }

    publish_bindings.sort_by_key(|binding| (binding.consumption.dispatch_rank(), binding.binding_id));

    Ok(publish_bindings)
}

fn classify_publish_binding_consumption(
    publish_target_kind: &str,
    binding_options_json: &str,
) -> io::Result<PublishBindingConsumption> {
    if !publish_target_kind.trim().eq_ignore_ascii_case("filesystem") {
        return Ok(PublishBindingConsumption::NonConsuming);
    }

    let raw = binding_options_json.trim();
    if raw.is_empty() {
        return Ok(PublishBindingConsumption::NonConsuming);
    }

    let parsed = serde_json::from_str::<serde_json::Value>(raw)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let operation = parsed
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();

    if operation.eq_ignore_ascii_case("move") {
        return Ok(PublishBindingConsumption::Consuming);
    }

    Ok(PublishBindingConsumption::NonConsuming)
}

fn list_existing_publish_run_keys(
    connection: &Connection,
    build_run_id: i64,
) -> io::Result<HashSet<(i64, i64)>> {
    let mut statement = connection
        .prepare(
            "
            SELECT publish_target_id,
                   artifact_id
            FROM publish_runs
            WHERE build_run_id = ?
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_run_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(sqlite_error)?;

    let mut existing = HashSet::new();
    for row in rows {
        let (publish_target_id, artifact_id) = row.map_err(sqlite_error)?;
        if let Some(artifact_id) = artifact_id {
            existing.insert((publish_target_id, artifact_id));
        }
    }

    Ok(existing)
}

fn list_publish_runs_with_connection(
    connection: &Connection,
    build_run_id: i64,
) -> io::Result<Vec<PublishRunRecord>> {
    let mut statement = connection
        .prepare(
            "
            SELECT id,
                   release_run_id,
                   build_run_id,
                   publish_target_id,
                   artifact_id,
                   status,
                   destination_ref,
                     execution_contract_json,
                   started_at,
                   finished_at,
                   error_message,
                   created_at,
                   updated_at
            FROM publish_runs
            WHERE build_run_id = ?
            ORDER BY artifact_id ASC,
                     id ASC
            ",
        )
        .map_err(sqlite_error)?;
    let rows = statement
        .query_map([build_run_id], scan_publish_run_record)
        .map_err(sqlite_error)?;

    let mut runs = Vec::new();
    for row in rows {
        runs.push(row.map_err(sqlite_error)?);
    }

    Ok(runs)
}

fn scan_artifact_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactRecord> {
    let path = row.get::<_, String>(4)?.trim().to_owned();
    let (active_location_kind, active_location_ref) =
        scan_artifact_active_location_columns(row.get(5)?, row.get(6)?, path.as_str());

    Ok(ArtifactRecord {
        id: row.get(0)?,
        build_run_id: row.get(1)?,
        name: row.get::<_, String>(2)?.trim().to_owned(),
        kind: row.get::<_, String>(3)?.trim().to_owned(),
        path,
        active_location_kind,
        active_location_ref,
        size_bytes: row.get(7)?,
        checksum_sha256: normalize_optional_string(row.get(8)?),
        created_at: row.get(9)?,
    })
}

fn scan_publish_run_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<PublishRunRecord> {
    Ok(PublishRunRecord {
        id: row.get(0)?,
        release_run_id: row.get(1)?,
        build_run_id: row.get(2)?,
        publish_target_id: row.get(3)?,
        artifact_id: row.get(4)?,
        status: row.get(5)?,
        destination_ref: normalize_optional_string(row.get(6)?),
        execution_contract_json: row.get(7)?,
        started_at: normalize_optional_string(row.get(8)?),
        finished_at: normalize_optional_string(row.get(9)?),
        error_message: normalize_optional_string(row.get(10)?),
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn scan_publish_execution_plan(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PublishExecutionPlan> {
    let execution_contract_json = row.get::<_, String>(10)?;
    let target_snapshot =
        scan_publish_execution_target_snapshot(execution_contract_json.as_str()).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                10,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let artifact_id = row.get::<_, Option<i64>>(12)?.ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            12,
            rusqlite::types::Type::Integer,
            Box::new(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "publish run {} is missing artifact metadata",
                    row.get::<_, i64>(0).unwrap_or_default()
                ),
            )),
        )
    })?;
    let artifact_name = row
        .get::<_, Option<String>>(13)?
        .and_then(|value| normalize_optional_string(Some(value)))
        .ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "publish run {} is missing artifact name",
                        row.get::<_, i64>(0).unwrap_or_default()
                    ),
                )),
            )
        })?;
    let artifact_kind = row
        .get::<_, Option<String>>(14)?
        .and_then(|value| normalize_optional_string(Some(value)))
        .unwrap_or_else(|| String::from("file"));
    let artifact_path = normalize_relative_store_artifact_path(
        &row
            .get::<_, Option<String>>(15)?
            .and_then(|value| normalize_optional_string(Some(value)))
            .ok_or_else(
            || {
                rusqlite::Error::FromSqlConversionFailure(
                    15,
                    rusqlite::types::Type::Text,
                    Box::new(io::Error::new(
                        ErrorKind::InvalidInput,
                        format!(
                            "publish run {} is missing artifact path",
                            row.get::<_, i64>(0).unwrap_or_default()
                        ),
                    )),
                )
            },
        )?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                15,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let artifact_root_path = row
        .get::<_, Option<String>>(16)?
        .and_then(|value| normalize_optional_string(Some(value)))
        .ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                16,
                rusqlite::types::Type::Text,
                Box::new(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "publish run {} is missing build artifact root path",
                        row.get::<_, i64>(0).unwrap_or_default()
                    ),
                )),
            )
        })?;
    let (artifact_active_location_kind, artifact_active_location_ref) =
        scan_artifact_active_location_columns(row.get(17)?, row.get(18)?, artifact_path.as_str());
    let source_path = resolve_artifact_source_path(
        artifact_active_location_kind.as_str(),
        artifact_active_location_ref.as_str(),
        artifact_root_path.as_str(),
    )
    .map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            18,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;

    Ok(PublishExecutionPlan {
        publish_run_id: row.get(0)?,
        release_run_id: row.get(1)?,
        repository_id: row.get(2)?,
        repository_name: row.get::<_, String>(3)?.trim().to_owned(),
        git_tag: row.get::<_, String>(4)?.trim().to_owned(),
        build_run_id: row.get(5)?,
        publish_target_id: row.get(6)?,
        publish_target_name: if target_snapshot.publish_target_name.trim().is_empty() {
            row.get::<_, String>(7)?.trim().to_owned()
        } else {
            target_snapshot.publish_target_name.trim().to_owned()
        },
        publish_target_kind: if target_snapshot.publish_target_kind.trim().is_empty() {
            row.get::<_, String>(8)?.trim().to_owned()
        } else {
            target_snapshot.publish_target_kind.trim().to_owned()
        },
        publish_target_config_json: if target_snapshot
            .publish_target_config_json
            .trim()
            .is_empty()
        {
            row.get(9)?
        } else {
            target_snapshot.publish_target_config_json.clone()
        },
        publish_target_credentials_id: target_snapshot.publish_target_credentials_id,
        execution_contract_json,
        status: row.get(11)?,
        artifact_id,
        artifact_name,
        artifact_kind,
        artifact_path,
        artifact_active_location_kind,
        artifact_active_location_ref,
        artifact_root_path,
        source_path,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
struct PublishExecutionTargetSnapshot {
    #[serde(default)]
    publish_target_name: String,
    #[serde(default)]
    publish_target_kind: String,
    #[serde(default)]
    publish_target_config_json: String,
    #[serde(default)]
    publish_target_credentials_id: Option<i64>,
}

fn scan_publish_execution_target_snapshot(
    execution_contract_json: &str,
) -> io::Result<PublishExecutionTargetSnapshot> {
    let trimmed = execution_contract_json.trim();
    if trimmed.is_empty() {
        return Ok(PublishExecutionTargetSnapshot::default());
    }

    let parsed = serde_json::from_str::<PublishExecutionTargetSnapshot>(trimmed)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    Ok(parsed)
}

fn scan_artifact_active_location_columns(
    kind: Option<String>,
    reference: Option<String>,
    artifact_path: &str,
) -> (String, String) {
    let normalized_kind = normalize_optional_string(kind)
        .unwrap_or_else(|| String::from("runtime_artifact"));
    let normalized_ref = normalize_optional_string(reference)
        .unwrap_or_else(|| artifact_path.trim().to_owned());

    (normalized_kind, normalized_ref)
}

fn resolve_artifact_source_path(
    active_location_kind: &str,
    active_location_ref: &str,
    artifact_root_path: &str,
) -> io::Result<String> {
    match active_location_kind.trim() {
        "" | "runtime_artifact" => {
            let relative_path = normalize_relative_store_artifact_path(active_location_ref)?;
            Ok(Path::new(artifact_root_path)
                .join(relative_path.replace('/', &std::path::MAIN_SEPARATOR.to_string()))
                .display()
                .to_string())
        }
        "filesystem_absolute" => {
            let absolute_path = PathBuf::from(active_location_ref.trim());
            if absolute_path.as_os_str().is_empty() {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "filesystem absolute active location must not be empty",
                ));
            }
            if !absolute_path.is_absolute() {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "filesystem absolute active location must be absolute",
                ));
            }

            Ok(absolute_path.display().to_string())
        }
        other => Err(io::Error::new(
            ErrorKind::InvalidInput,
            format!("unsupported artifact active location kind {other:?}"),
        )),
    }
}

fn build_publish_run_execution_contract_json(
    connection: &Connection,
    publish_binding: &PublishBindingPlanningRecord,
    artifact_id: i64,
) -> io::Result<String> {
    let snapshot = connection
        .query_row(
            "
            SELECT a.path,
                   a.active_location_kind,
                   a.active_location_ref
            FROM artifacts a
            WHERE a.id = ?
            ",
            [artifact_id],
            |row| {
                let artifact_path = row.get::<_, String>(0)?.trim().to_owned();
                let (active_location_kind, active_location_ref) =
                    scan_artifact_active_location_columns(row.get(1)?, row.get(2)?, artifact_path.as_str());

                Ok(serde_json::json!({
                    "publish_target_id": publish_binding.publish_target_id,
                    "publish_target_credentials_id": publish_binding.publish_target_credentials_id,
                    "artifact_id": artifact_id,
                    "publish_target_name": publish_binding.publish_target_name,
                    "publish_target_kind": publish_binding.publish_target_kind,
                    "publish_target_config_json": publish_binding.publish_target_config_json,
                    "binding_id": publish_binding.binding_id,
                    "binding_options_json": publish_binding.binding_options_json,
                    "consumption_behavior": publish_binding.consumption.as_str(),
                    "dispatch_rank": publish_binding.consumption.dispatch_rank(),
                    "artifact_path": artifact_path,
                    "artifact_active_location_kind": active_location_kind,
                    "artifact_active_location_ref": active_location_ref,
                }))
            },
        )
        .optional()
        .map_err(sqlite_error)?
        .ok_or_else(|| not_found_error(format!("artifact {artifact_id} was not found")))?;

    serde_json::to_string(&snapshot)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn has_incomplete_prior_publish_run_for_artifact(
    transaction: &rusqlite::Transaction<'_>,
    publish_run_id: i64,
    artifact_id: i64,
) -> io::Result<bool> {
    let blocked_count: i64 = transaction
        .query_row(
            "
            SELECT COUNT(1)
            FROM publish_runs
            WHERE artifact_id = ?
              AND id < ?
              AND status IN (?, ?)
            ",
            params![
                artifact_id,
                publish_run_id,
                PublishStatus::Queued.as_str(),
                PublishStatus::Running.as_str(),
            ],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;

    Ok(blocked_count > 0)
}

fn normalize_relative_store_artifact_path(path: &str) -> io::Result<String> {
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

fn scan_build_execution_plan(row: &rusqlite::Row<'_>) -> rusqlite::Result<BuildExecutionPlan> {
    let engine_version = row
        .get::<_, Option<String>>(22)?
        .unwrap_or_default()
        .trim()
        .to_owned();
    let image_ref = row
        .get::<_, Option<String>>(23)?
        .unwrap_or_default()
        .trim()
        .to_owned();
    if engine_version.is_empty() || image_ref.is_empty() {
        return Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "build run {} is missing planned image metadata",
                    row.get::<_, i64>(0)?
                ),
            )),
        ));
    }

    Ok(BuildExecutionPlan {
        build_run_id: row.get(0)?,
        release_run_id: row.get(1)?,
        repository_id: row.get(2)?,
        engine_kind: parse_engine_kind_sql(3, row.get::<_, String>(3)?)?,
        repository_name: row.get::<_, String>(4)?.trim().to_owned(),
        repository_credentials_id: row.get(5)?,
        workspace_root_override: normalize_optional_string(row.get(6)?),
        artifacts_root_override: normalize_optional_string(row.get(7)?),
        build_target_id: row.get(8)?,
        repository_source_mode: row.get::<_, String>(9)?.trim().to_owned(),
        repository_url: row
            .get::<_, Option<String>>(10)?
            .unwrap_or_default()
            .trim()
            .to_owned(),
        repository_local_path: normalize_optional_string(row.get(11)?),
        git_tag: row.get::<_, String>(12)?.trim().to_owned(),
        git_commit: normalize_optional_string(row.get(13)?),
        source_metadata_json: row.get(14)?,
        target_name: row.get::<_, String>(15)?.trim().to_owned(),
        build_kind: parse_build_kind_sql(16, row.get::<_, String>(16)?)?,
        contract_json: row.get(17)?,
        runner_type: row.get::<_, String>(18)?.trim().to_owned(),
        output_kind: normalize_optional_string(row.get(19)?),
        output_path_template: normalize_optional_string(row.get(20)?),
        config_json: row.get(21)?,
        engine_version,
        image_ref,
        timeout_seconds: row.get(24)?,
        status: row.get(25)?,
    })
}

fn scan_credential_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialRecord> {
    Ok(CredentialRecord {
        id: row.get(0)?,
        name: row.get::<_, String>(1)?.trim().to_owned(),
        kind: row.get::<_, String>(2)?.trim().to_owned(),
        config_json: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn normalize_artifact_record_input(
    input: CreateArtifactRecordInput,
) -> io::Result<CreateArtifactRecordInput> {
    let name = require_non_empty(&input.name, "artifact name")?;
    let kind = require_non_empty(&input.kind, "artifact kind")?;
    let path = require_non_empty(&input.path, "artifact path")?;
    if input.size_bytes.is_some_and(|size_bytes| size_bytes < 0) {
        return Err(invalid_input_error("artifact size must not be negative"));
    }

    Ok(CreateArtifactRecordInput {
        name,
        kind,
        path,
        size_bytes: input.size_bytes,
        checksum_sha256: input
            .checksum_sha256
            .and_then(|value| normalize_optional_string(Some(value))),
    })
}

fn scan_release_build_planning_state(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<ReleaseBuildPlanningState> {
    Ok(ReleaseBuildPlanningState {
        repository_id: row.get(0)?,
        repository_source_mode: row.get::<_, String>(1)?.trim().to_owned(),
        repository_url: row
            .get::<_, Option<String>>(2)?
            .unwrap_or_default()
            .trim()
            .to_owned(),
        repository_local_path: normalize_optional_string(row.get(3)?),
        engine_kind: parse_engine_kind_sql(4, row.get::<_, String>(4)?)?,
        credentials_id: row.get(5)?,
        git_tag: row.get::<_, String>(6)?.trim().to_owned(),
        source_metadata_json: row.get(7)?,
        engine_version: normalize_optional_string(row.get(8)?),
        status: row.get(9)?,
    })
}

impl From<&ReleaseRunRecord> for ReleaseDispatchJob {
    fn from(record: &ReleaseRunRecord) -> Self {
        Self {
            release_run_id: record.id,
            repository_id: record.repository_id,
            git_tag: record.git_tag.clone(),
            git_commit: record.git_commit.clone(),
            trigger_source: record.trigger_source.clone(),
            trigger_rule_id: record.trigger_rule_id,
        }
    }
}

fn require_non_empty(value: &str, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(invalid_input_error(format!("{label} must not be empty")));
    }

    Ok(trimmed.to_owned())
}

fn normalize_upsert_credential_record_input(
    input: UpsertCredentialRecordInput,
) -> io::Result<UpsertCredentialRecordInput> {
    if let Some(credential_id) = input.credential_id {
        require_positive_identifier(credential_id, "credentials id")?;
    }

    let name = require_non_empty(&input.name, "credentials name")?;
    let kind = require_non_empty(&input.kind, "credentials kind")?;
    let config_json = normalize_credential_config_json(&input.config_json)?;

    Ok(UpsertCredentialRecordInput {
        credential_id: input.credential_id,
        name,
        kind,
        config_json,
    })
}

fn normalize_create_repository_project_input(
    input: CreateRepositoryProjectInput,
) -> io::Result<CreateRepositoryProjectInput> {
    let name = require_non_empty(&input.name, "repository project name")?;
    let engine_kind = normalize_repository_project_engine_kind(&input.engine_kind)?;
    let source_mode = require_non_empty(&input.source_mode, "repository project source_mode")?
        .to_ascii_lowercase();
    let repo_url = normalize_optional_string(input.repo_url);
    let local_path = normalize_absolute_path_string(
        input.local_path,
        "repository project local_path",
    )?;

    match source_mode.as_str() {
        SOURCE_MODE_MANAGED_REPOSITORY => {
            let repo_url = repo_url.as_deref().ok_or_else(|| {
                invalid_input_error("repository project repo_url must not be empty")
            })?;
            require_non_empty(repo_url, "repository project repo_url")?;
        }
        SOURCE_MODE_LOCAL_WORKSPACE => {
            let local_path = local_path.as_deref().ok_or_else(|| {
                invalid_input_error("repository project local_path must not be empty")
            })?;
            require_non_empty(local_path, "repository project local_path")?;
        }
        _ => {
            return Err(invalid_input_error(format!(
                "repository project source_mode {:?} is not supported",
                source_mode
            )));
        }
    }
    let default_branch = normalize_optional_input_string(input.default_branch);
    let artifacts_root_override = normalize_optional_input_string(input.artifacts_root_override);
    let workspace_root_override = normalize_optional_input_string(input.workspace_root_override);
    let polling_interval_seconds = normalize_repository_project_polling_interval_seconds(
        source_mode.as_str(),
        input.polling_interval_seconds,
    )?;
    if input.build_targets.is_empty() {
        return Err(invalid_input_error(
            "repository project must include at least one build target",
        ));
    }

    let credentials = input
        .credentials
        .map(normalize_create_repository_project_credentials_input)
        .transpose()?;
    let mut build_target_names = HashSet::new();
    let mut build_targets = Vec::with_capacity(input.build_targets.len());
    for target in input.build_targets {
        let normalized =
            normalize_create_repository_project_build_target_input(target, &engine_kind)?;
        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !build_target_names.insert(duplicate_key) {
            return Err(invalid_input_error(
                "repository project build target names must be unique",
            ));
        }
        build_targets.push(normalized);
    }

    let mut publish_target_names = HashSet::new();
    let mut publish_targets = Vec::with_capacity(input.publish_targets.len());
    for target in input.publish_targets {
        let normalized = normalize_create_repository_project_publish_target_input(
            target,
            &build_target_names,
        )?;
        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !publish_target_names.insert(duplicate_key) {
            return Err(invalid_input_error(
                "repository project publish target names must be unique",
            ));
        }
        publish_targets.push(normalized);
    }

    Ok(CreateRepositoryProjectInput {
        name,
        engine_kind,
        source_mode,
        repo_url,
        local_path,
        credentials,
        default_branch,
        artifacts_root_override,
        workspace_root_override,
        polling_interval_seconds,
        enabled: input.enabled,
        build_targets,
        publish_targets,
    })
}

fn normalize_update_repository_project_input(
    input: UpdateRepositoryProjectInput,
) -> io::Result<UpdateRepositoryProjectInput> {
    require_positive_identifier(input.repository_id, "repository id")?;

    let name = require_non_empty(&input.name, "repository project name")?;
    let engine_kind = normalize_repository_project_engine_kind(&input.engine_kind)?;
    let source_mode = require_non_empty(&input.source_mode, "repository project source_mode")?
        .to_ascii_lowercase();
    let repo_url = normalize_optional_string(input.repo_url);
    let local_path = normalize_absolute_path_string(
        input.local_path,
        "repository project local_path",
    )?;

    match source_mode.as_str() {
        SOURCE_MODE_MANAGED_REPOSITORY => {
            let repo_url = repo_url.as_deref().ok_or_else(|| {
                invalid_input_error("repository project repo_url must not be empty")
            })?;
            require_non_empty(repo_url, "repository project repo_url")?;
        }
        SOURCE_MODE_LOCAL_WORKSPACE => {
            let local_path = local_path.as_deref().ok_or_else(|| {
                invalid_input_error("repository project local_path must not be empty")
            })?;
            require_non_empty(local_path, "repository project local_path")?;
        }
        _ => {
            return Err(invalid_input_error(format!(
                "repository project source_mode {:?} is not supported",
                source_mode
            )));
        }
    }

    let default_branch = normalize_optional_input_string(input.default_branch);
    let artifacts_root_override = normalize_optional_input_string(input.artifacts_root_override);
    let workspace_root_override = normalize_optional_input_string(input.workspace_root_override);
    let polling_interval_seconds = normalize_repository_project_polling_interval_seconds(
        source_mode.as_str(),
        input.polling_interval_seconds,
    )?;

    if input.build_targets.is_empty() {
        return Err(invalid_input_error(
            "repository project must include at least one build target",
        ));
    }

    let mut build_target_ids = HashSet::new();
    let mut build_target_names = HashSet::new();
    let mut build_targets = Vec::with_capacity(input.build_targets.len());
    for target in input.build_targets {
        let normalized =
            normalize_update_repository_project_build_target_input(target, &engine_kind)?;

        if let Some(build_target_id) = normalized.build_target_id {
            if !build_target_ids.insert(build_target_id) {
                return Err(invalid_input_error(format!(
                    "repository project build target {build_target_id} was provided more than once"
                )));
            }
        }

        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !build_target_names.insert(duplicate_key) {
            return Err(invalid_input_error(
                "repository project build target names must be unique",
            ));
        }

        build_targets.push(normalized);
    }

    let build_target_names_by_id = build_targets
        .iter()
        .filter_map(|target| {
            target
                .build_target_id
                .map(|build_target_id| (build_target_id, target.name.clone()))
        })
        .collect::<HashMap<_, _>>();
    let mut publish_target_names = HashSet::new();
    let mut publish_targets = Vec::with_capacity(input.publish_targets.len());
    for target in input.publish_targets {
        let normalized = normalize_update_repository_project_publish_target_input(
            target,
            &build_target_ids,
            &build_target_names,
            &build_target_names_by_id,
        )?;
        let duplicate_key = normalized.name.to_ascii_lowercase();
        if !publish_target_names.insert(duplicate_key) {
            return Err(invalid_input_error(
                "repository project publish target names must be unique",
            ));
        }
        publish_targets.push(normalized);
    }

    Ok(UpdateRepositoryProjectInput {
        repository_id: input.repository_id,
        name,
        engine_kind,
        source_mode,
        repo_url,
        local_path,
        default_branch,
        artifacts_root_override,
        workspace_root_override,
        polling_interval_seconds,
        enabled: input.enabled,
        build_targets,
        publish_targets,
    })
}

fn normalize_repository_project_polling_interval_seconds(
    source_mode: &str,
    polling_interval_seconds: i64,
) -> io::Result<i64> {
    if source_mode == SOURCE_MODE_LOCAL_WORKSPACE {
        return Ok(0);
    }

    if polling_interval_seconds <= 0 {
        return Err(invalid_input_error(
            "repository project polling_interval_seconds must be greater than zero",
        ));
    }

    Ok(polling_interval_seconds)
}

fn project_polling_interval_seconds_for_read_model(
    source_mode: &str,
    polling_interval_seconds: i64,
) -> i64 {
    if source_mode == SOURCE_MODE_LOCAL_WORKSPACE {
        0
    } else {
        polling_interval_seconds
    }
}

fn normalize_create_repository_project_credentials_input(
    input: CreateRepositoryProjectCredentialInput,
) -> io::Result<CreateRepositoryProjectCredentialInput> {
    Ok(CreateRepositoryProjectCredentialInput {
        name: require_non_empty(&input.name, "repository project credentials name")?,
        kind: require_non_empty(&input.kind, "repository project credentials kind")?,
        config_json: normalize_credential_config_json(&input.config_json)?,
    })
}

fn normalize_create_repository_project_build_target_input(
    input: CreateRepositoryProjectBuildTargetInput,
    engine_kind: &str,
) -> io::Result<CreateRepositoryProjectBuildTargetInput> {
    if input.timeout_seconds <= 0 {
        return Err(invalid_input_error(
            "repository project build target timeout_seconds must be greater than zero",
        ));
    }

    let build_kind =
        normalize_repository_project_build_kind(&input.build_kind, engine_kind)?;
    let contract_json = normalize_required_object_json_string(
        &input.contract_json,
        "repository project build target contract_json",
    )?;
    project_repository_project_build_target_contract(
        engine_kind,
        &build_kind,
        &contract_json,
    )?;

    Ok(CreateRepositoryProjectBuildTargetInput {
        name: require_non_empty(&input.name, "repository project build target name")?,
        build_kind,
        runner_type: require_non_empty(
            &input.runner_type,
            "repository project build target runner_type",
        )?,
        output_kind: normalize_optional_input_string(input.output_kind),
        output_path_template: normalize_optional_input_string(input.output_path_template),
        timeout_seconds: input.timeout_seconds,
        enabled: input.enabled,
        contract_json,
        runner_config_json: normalize_required_object_json_string(
            &input.runner_config_json,
            "repository project build target runner_config_json",
        )?,
    })
}

fn normalize_create_repository_project_publish_target_input(
    input: CreateRepositoryProjectPublishTargetInput,
    build_target_names: &HashSet<String>,
) -> io::Result<CreateRepositoryProjectPublishTargetInput> {
    if let Some(credentials_id) = input.credentials_id {
        require_positive_identifier(credentials_id, "repository project publish target credentials id")?;
    }

    let name = require_non_empty(&input.name, "repository project publish target name")?;
    let kind = normalize_repository_project_publish_target_kind(&input.kind)?;
    let config_json = normalize_required_object_json_string(
        &input.config_json,
        "repository project publish target config_json",
    )?;
    validate_repository_project_publish_target_config(kind.as_str(), config_json.as_str())?;

    let mut claimed_build_targets = HashSet::new();
    let mut bindings = Vec::with_capacity(input.bindings.len());
    for binding in input.bindings {
        let normalized = normalize_create_repository_project_publish_binding_input(
            binding,
            kind.as_str(),
            build_target_names,
        )?;
        let duplicate_key = normalized.build_target_name.to_ascii_lowercase();
        if !claimed_build_targets.insert(duplicate_key) {
            return Err(invalid_input_error(format!(
                "repository project publish target {:?} cannot bind the same build target more than once",
                name
            )));
        }
        bindings.push(normalized);
    }

    Ok(CreateRepositoryProjectPublishTargetInput {
        name,
        kind,
        enabled: input.enabled,
        config_json,
        credentials_id: input.credentials_id,
        bindings,
    })
}

fn normalize_create_repository_project_publish_binding_input(
    input: CreateRepositoryProjectPublishBindingInput,
    publish_target_kind: &str,
    build_target_names: &HashSet<String>,
) -> io::Result<CreateRepositoryProjectPublishBindingInput> {
    let build_target_name = require_non_empty(
        &input.build_target_name,
        "repository project publish binding build_target_name",
    )?;
    if !build_target_names.contains(&build_target_name.to_ascii_lowercase()) {
        return Err(not_found_error(format!(
            "repository project publish binding build target {:?} was not declared by this project",
            build_target_name
        )));
    }

    let options_json = normalize_required_object_json_string(
        &input.options_json,
        "repository project publish binding options_json",
    )?;
    validate_repository_project_publish_binding_options(
        publish_target_kind,
        options_json.as_str(),
    )?;

    Ok(CreateRepositoryProjectPublishBindingInput {
        build_target_name,
        enabled: input.enabled,
        options_json,
    })
}

fn normalize_update_repository_project_build_target_input(
    input: UpdateRepositoryProjectBuildTargetInput,
    engine_kind: &str,
) -> io::Result<UpdateRepositoryProjectBuildTargetInput> {
    if let Some(build_target_id) = input.build_target_id {
        require_positive_identifier(build_target_id, "repository project build target id")?;
    }

    let normalized_target = normalize_create_repository_project_build_target_input(
        CreateRepositoryProjectBuildTargetInput {
            name: input.name,
            build_kind: input.build_kind,
            runner_type: input.runner_type,
            output_kind: input.output_kind,
            output_path_template: input.output_path_template,
            timeout_seconds: input.timeout_seconds,
            enabled: input.enabled,
            contract_json: input.contract_json,
            runner_config_json: input.runner_config_json,
        },
        engine_kind,
    )?;

    Ok(UpdateRepositoryProjectBuildTargetInput {
        build_target_id: input.build_target_id,
        name: normalized_target.name,
        build_kind: normalized_target.build_kind,
        runner_type: normalized_target.runner_type,
        output_kind: normalized_target.output_kind,
        output_path_template: normalized_target.output_path_template,
        timeout_seconds: normalized_target.timeout_seconds,
        enabled: normalized_target.enabled,
        contract_json: normalized_target.contract_json,
        runner_config_json: normalized_target.runner_config_json,
    })
}

fn normalize_update_repository_project_publish_target_input(
    input: UpdateRepositoryProjectPublishTargetInput,
    build_target_ids: &HashSet<i64>,
    build_target_names: &HashSet<String>,
    build_target_names_by_id: &HashMap<i64, String>,
) -> io::Result<UpdateRepositoryProjectPublishTargetInput> {
    if let Some(publish_target_id) = input.publish_target_id {
        require_positive_identifier(publish_target_id, "repository project publish target id")?;
    }
    if let Some(credentials_id) = input.credentials_id {
        require_positive_identifier(credentials_id, "repository project publish target credentials id")?;
    }

    let name = require_non_empty(&input.name, "repository project publish target name")?;
    let kind = normalize_repository_project_publish_target_kind(&input.kind)?;
    let config_json = normalize_required_object_json_string(
        &input.config_json,
        "repository project publish target config_json",
    )?;
    validate_repository_project_publish_target_config(kind.as_str(), config_json.as_str())?;

    let mut claimed_build_targets = HashSet::new();
    let mut bindings = Vec::with_capacity(input.bindings.len());
    for binding in input.bindings {
        let normalized = normalize_update_repository_project_publish_binding_input(
            binding,
            kind.as_str(),
            build_target_ids,
            build_target_names,
            build_target_names_by_id,
        )?;
        let duplicate_key = normalized
            .build_target_id
            .map(|build_target_id| format!("id:{build_target_id}"))
            .unwrap_or_else(|| normalized.build_target_name.to_ascii_lowercase());
        if !claimed_build_targets.insert(duplicate_key) {
            return Err(invalid_input_error(format!(
                "repository project publish target {:?} cannot bind the same build target more than once",
                name
            )));
        }
        bindings.push(normalized);
    }

    Ok(UpdateRepositoryProjectPublishTargetInput {
        publish_target_id: input.publish_target_id,
        name,
        kind,
        enabled: input.enabled,
        config_json,
        credentials_id: input.credentials_id,
        bindings,
    })
}

fn normalize_update_repository_project_publish_binding_input(
    input: UpdateRepositoryProjectPublishBindingInput,
    publish_target_kind: &str,
    build_target_ids: &HashSet<i64>,
    build_target_names: &HashSet<String>,
    build_target_names_by_id: &HashMap<i64, String>,
) -> io::Result<UpdateRepositoryProjectPublishBindingInput> {
    if let Some(build_target_id) = input.build_target_id {
        require_positive_identifier(build_target_id, "repository project publish binding build_target_id")?;
        if !build_target_ids.contains(&build_target_id) {
            return Err(not_found_error(format!(
                "repository project publish binding build target {build_target_id} was not declared by this project"
            )));
        }
    }

    let build_target_name = require_non_empty(
        &input.build_target_name,
        "repository project publish binding build_target_name",
    )?;
    if let Some(build_target_id) = input.build_target_id {
        if let Some(expected_name) = build_target_names_by_id.get(&build_target_id) {
            if !expected_name.eq_ignore_ascii_case(build_target_name.as_str()) {
                return Err(invalid_input_error(format!(
                    "repository project publish binding build target {:?} does not match build target {build_target_id}",
                    build_target_name
                )));
            }
        }
    } else if !build_target_names.contains(&build_target_name.to_ascii_lowercase()) {
        return Err(not_found_error(format!(
            "repository project publish binding build target {:?} was not declared by this project",
            build_target_name
        )));
    }

    let options_json = normalize_required_object_json_string(
        &input.options_json,
        "repository project publish binding options_json",
    )?;
    validate_repository_project_publish_binding_options(
        publish_target_kind,
        options_json.as_str(),
    )?;

    Ok(UpdateRepositoryProjectPublishBindingInput {
        build_target_id: input.build_target_id,
        build_target_name,
        enabled: input.enabled,
        options_json,
    })
}

fn normalize_repository_project_publish_target_kind(kind: &str) -> io::Result<String> {
    let normalized = require_non_empty(kind, "repository project publish target kind")?
        .to_ascii_lowercase();
    if normalized == "filesystem" || normalized == "itch" {
        return Ok(normalized);
    }

    Err(invalid_input_error(format!(
        "repository project publish target kind {:?} is not supported",
        normalized
    )))
}

fn validate_repository_project_publish_target_config(
    publish_target_kind: &str,
    config_json: &str,
) -> io::Result<()> {
    let parsed = serde_json::from_str::<serde_json::Value>(config_json)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let object = parsed.as_object().ok_or_else(|| {
        invalid_input_error("repository project publish target config_json must decode to a JSON object")
    })?;

    match publish_target_kind.trim() {
        "filesystem" => {
            if !object.is_empty() {
                return Err(invalid_input_error(
                    "filesystem publish target config_json must be an empty JSON object in the current slice",
                ));
            }
            Ok(())
        }
        "itch" => {
            require_json_object_string_field(
                object,
                "account_name",
                "itch publish target config_json account_name",
            )?;
            require_json_object_string_field(
                object,
                "game_slug",
                "itch publish target config_json game_slug",
            )?;
            if let Some(butler_path) = object.get("butler_path").and_then(serde_json::Value::as_str) {
                require_non_empty(butler_path, "itch publish target config_json butler_path")?;
            }
            Ok(())
        }
        _ => Err(invalid_input_error(format!(
            "repository project publish target kind {:?} is not supported",
            publish_target_kind
        ))),
    }
}

fn validate_repository_project_publish_binding_options(
    publish_target_kind: &str,
    options_json: &str,
) -> io::Result<()> {
    let parsed = serde_json::from_str::<serde_json::Value>(options_json)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let object = parsed.as_object().ok_or_else(|| {
        invalid_input_error("repository project publish binding options_json must decode to a JSON object")
    })?;

    match publish_target_kind.trim() {
        "filesystem" => {
            let operation = require_json_object_string_field(
                object,
                "operation",
                "filesystem publish binding operation",
            )?;
            if !operation.eq_ignore_ascii_case("move") {
                return Err(invalid_input_error(
                    "filesystem publish bindings only support the move operation in the current slice",
                ));
            }
            let directory_path = require_json_object_string_field(
                object,
                "directory_path",
                "filesystem publish binding directory_path",
            )?;
            if !Path::new(directory_path.as_str()).is_absolute() {
                return Err(invalid_input_error(
                    "filesystem publish binding directory_path must be an absolute path",
                ));
            }
            Ok(())
        }
        "itch" => {
            let channel = require_json_object_string_field(
                object,
                "channel",
                "itch publish binding channel",
            )?;
            if channel.contains(':') {
                return Err(invalid_input_error(
                    "itch publish binding channel must not contain ':'",
                ));
            }
            Ok(())
        }
        _ => Err(invalid_input_error(format!(
            "repository project publish target kind {:?} is not supported",
            publish_target_kind
        ))),
    }
}

fn require_json_object_string_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field_name: &str,
    label: &str,
) -> io::Result<String> {
    let value = object
        .get(field_name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_input_error(format!("{label} is required")))?;

    require_non_empty(value, label)
}

fn validate_repository_project_publish_target_credentials_binding(
    connection: &Connection,
    credentials_id: Option<i64>,
    publish_target_kind: &str,
) -> io::Result<()> {
    validate_optional_credentials_binding(connection, credentials_id)?;
    let Some(credentials_id) = credentials_id else {
        return Ok(());
    };

    if !publish_target_kind.trim().eq_ignore_ascii_case("itch") {
        return Ok(());
    }

    let credentials_kind = connection
        .query_row(
            "SELECT kind FROM credentials WHERE id = ?",
            [credentials_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(sqlite_error)?;
    if credentials_kind.trim() != KIND_ITCH_API_KEY {
        return Err(invalid_input_error(format!(
            "itch publish targets require credentials kind {KIND_ITCH_API_KEY:?}"
        )));
    }

    Ok(())
}

fn normalize_credential_config_json(config_json: &str) -> io::Result<String> {
    let config_json = require_non_empty(config_json, "credentials config_json")?;
    let parsed = serde_json::from_str::<serde_json::Value>(&config_json).map_err(|error| {
        invalid_input_error(format!(
            "credentials config_json must be valid JSON: {error}"
        ))
    })?;

    if !parsed.is_object() {
        return Err(invalid_input_error(
            "credentials config_json must decode to a JSON object",
        ));
    }

    Ok(config_json)
}

fn normalize_repository_project_engine_kind(engine_kind: &str) -> io::Result<String> {
    let normalized = EngineKind::parse(&require_non_empty(
        engine_kind,
        "repository project engine_kind",
    )?)
    .map_err(|error| invalid_input_error(error.to_string()))?;
    if normalized != EngineKind::Unity {
        return Err(invalid_input_error(format!(
            "repository project engine_kind {:?} is not supported; expected \"unity\"",
            normalized.as_str()
        )));
    }

    Ok(String::from(normalized))
}

fn normalize_repository_project_build_kind(
    build_kind: &str,
    engine_kind: &str,
) -> io::Result<String> {
    let repository_engine_kind = EngineKind::parse(engine_kind)
        .map_err(|error| invalid_input_error(error.to_string()))?;
    let normalized = BuildKind::parse_or_default(build_kind)
        .map_err(|error| invalid_input_error(error.to_string()))?;

    match repository_engine_kind {
        EngineKind::Unity if normalized == BuildKind::Player => Ok(String::from(normalized)),
        EngineKind::Unity => Err(invalid_input_error(format!(
            "repository project build target build_kind {:?} is not supported for engine {:?}; expected \"player\"",
            normalized.as_str(),
            repository_engine_kind.as_str()
        ))),
        other => Err(invalid_input_error(format!(
            "repository project engine_kind {:?} is not supported",
            other.as_str()
        ))),
    }
}

fn parse_engine_kind_sql(column_index: usize, value: String) -> rusqlite::Result<EngineKind> {
    EngineKind::parse(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column_index,
            rusqlite::types::Type::Text,
            Box::new(io::Error::new(ErrorKind::InvalidInput, error.to_string())),
        )
    })
}

fn parse_build_kind_sql(column_index: usize, value: String) -> rusqlite::Result<BuildKind> {
    BuildKind::parse_or_default(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column_index,
            rusqlite::types::Type::Text,
            Box::new(io::Error::new(ErrorKind::InvalidInput, error.to_string())),
        )
    })
}

fn normalize_required_object_json_string(value: &str, label: &str) -> io::Result<String> {
    let value = require_non_empty(value, label)?;
    let parsed = serde_json::from_str::<serde_json::Value>(&value).map_err(|error| {
        invalid_input_error(format!("{label} must be valid JSON: {error}"))
    })?;
    if !parsed.is_object() {
        return Err(invalid_input_error(format!(
            "{label} must decode to a JSON object"
        )));
    }

    Ok(value)
}

fn project_repository_project_build_target_contract(
    engine_kind: &str,
    build_kind: &str,
    contract_json: &str,
) -> io::Result<BuildTargetReadModelProjection> {
    match engine_kind {
        SUPPORTED_REPOSITORY_ENGINE_UNITY => {
            if build_kind != SUPPORTED_REPOSITORY_BUILD_KIND_PLAYER {
                return Err(invalid_input_error(format!(
                    "repository project build target build_kind {:?} is not supported for engine \"unity\"",
                    build_kind
                )));
            }

            let contract = serde_json::from_str::<RepositoryProjectBuildContractInput>(contract_json)
                .map_err(|error| {
                    invalid_input_error(format!(
                        "repository project build target contract_json must match the supported engine contract schema: {error}"
                    ))
                })?;
            let unity = contract.unity.ok_or_else(|| {
                invalid_input_error(
                    "repository project build target contract_json must define contract.unity for engine \"unity\"",
                )
            })?;

            Ok(BuildTargetReadModelProjection {
                unity_target_platform: require_non_empty(
                    &unity.target_platform,
                    "repository project build target contract.unity.targetPlatform",
                )?,
                unity_build_method: Some(require_non_empty(
                    &unity.build_method,
                    "repository project build target contract.unity.buildMethod",
                )?),
            })
        }
        other => Err(invalid_input_error(format!(
            "repository project engine_kind {:?} is not supported",
            other
        ))),
    }
}

fn resolve_build_target_read_model_projection(
    engine_kind: &str,
    build_kind: &str,
    contract_json: &str,
) -> io::Result<BuildTargetReadModelProjection> {
    project_repository_project_build_target_contract(
        engine_kind,
        build_kind,
        contract_json,
    )
}

fn normalize_optional_input_string(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn reject_duplicate_repository_project_name(
    transaction: &Transaction<'_>,
    repository_name: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE name = ?)",
            [repository_name],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("repository project {:?} already exists", repository_name),
        ));
    }

    Ok(())
}

fn reject_duplicate_repository_project_name_for_update(
    transaction: &Transaction<'_>,
    repository_id: i64,
    repository_name: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE name = ? AND id <> ?)",
            params![repository_name, repository_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("repository project {:?} already exists", repository_name),
        ));
    }

    Ok(())
}

fn reject_duplicate_repository_project_url(
    transaction: &Transaction<'_>,
    repo_url: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE repo_url = ?)",
            [repo_url],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("repository project URL {:?} is already registered", repo_url),
        ));
    }

    Ok(())
}

fn reject_duplicate_repository_project_local_path(
    transaction: &Transaction<'_>,
    local_path: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE local_path = ?)",
            [local_path],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "repository project local_path {:?} is already registered",
                local_path
            ),
        ));
    }

    Ok(())
}

fn reject_duplicate_repository_project_url_for_update(
    transaction: &Transaction<'_>,
    repository_id: i64,
    repo_url: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE repo_url = ? AND id <> ?)",
            params![repo_url, repository_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!("repository project URL {:?} is already registered", repo_url),
        ));
    }

    Ok(())
}

fn reject_duplicate_repository_project_local_path_for_update(
    transaction: &Transaction<'_>,
    repository_id: i64,
    local_path: &str,
) -> io::Result<()> {
    let exists: i64 = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repositories WHERE local_path = ? AND id <> ?)",
            params![local_path, repository_id],
            |row| row.get(0),
        )
        .map_err(sqlite_error)?;
    if exists != 0 {
        return Err(io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "repository project local_path {:?} is already registered",
                local_path
            ),
        ));
    }

    Ok(())
}

pub fn store_host_secret(account: &str, secret_value: &str) -> io::Result<String> {
    let account = require_non_empty(account, "host keyring account")?;
    let secret_value = require_non_empty(secret_value, "host keyring secret value")?;
    let entry = open_host_keyring_entry(HOST_KEYRING_SERVICE, &account)?;
    entry.set_password(&secret_value).map_err(keyring_error)?;

    Ok(format!(
        "{KEYRING_SECRET_REF_PREFIX}{HOST_KEYRING_SERVICE}/{account}"
    ))
}

pub fn delete_host_secret(secret_ref: &str) -> io::Result<()> {
    let (service, account) = parse_host_secret_reference(secret_ref)?;
    let entry = open_host_keyring_entry(&service, &account)?;
    entry.delete_credential().map_err(keyring_error)
}

/// Resolves host-keyring-backed secret references inside one credential config JSON blob.
pub fn resolve_credential_secret_config_json(
    kind: &str,
    config_json: &str,
) -> io::Result<String> {
    let config_json = require_non_empty(config_json, "credentials config_json")?;
    let mut parsed = serde_json::from_str::<serde_json::Value>(&config_json)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
    let Some(object) = parsed.as_object_mut() else {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "credentials config_json must decode to a JSON object",
        ));
    };

    for key in credential_secret_value_keys(kind) {
        let Some(secret_value) = object.get(*key).and_then(|value| value.as_str()) else {
            continue;
        };

        object.insert(
            String::from(*key),
            serde_json::Value::String(resolve_host_secret_reference(secret_value)?),
        );
    }

    serde_json::to_string(&parsed).map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

fn credential_secret_value_keys(kind: &str) -> &'static [&'static str] {
    match kind.trim() {
        KIND_GIT_HTTP_BASIC => &["password"],
        KIND_GIT_HTTP_BEARER => &["token"],
        KIND_ITCH_API_KEY => &["api_key"],
        _ => &[],
    }
}

fn resolve_host_secret_reference(secret_ref: &str) -> io::Result<String> {
    let secret_ref = require_non_empty(secret_ref, "host keyring secret reference")?;
    let Some(reference_tail) = secret_ref.strip_prefix(KEYRING_SECRET_REF_PREFIX) else {
        return Ok(secret_ref.to_owned());
    };
    let (service, account) = reference_tail.split_once('/').ok_or_else(|| {
        invalid_input_error(
            "host keyring secret reference must follow keyring://<service>/<account>",
        )
    })?;
    let service = require_non_empty(service, "host keyring service")?;
    let account = require_non_empty(account, "host keyring account")?;
    let entry = open_host_keyring_entry(&service, &account)?;
    let secret_value = entry.get_password().map_err(keyring_error)?;

    require_non_empty(&secret_value, "resolved host keyring secret value")
}

fn parse_host_secret_reference(secret_ref: &str) -> io::Result<(String, String)> {
    let secret_ref = require_non_empty(secret_ref, "host keyring secret reference")?;
    let Some(reference_tail) = secret_ref.strip_prefix(KEYRING_SECRET_REF_PREFIX) else {
        return Err(invalid_input_error(
            "host keyring secret reference must begin with keyring://",
        ));
    };
    let (service, account) = reference_tail.split_once('/').ok_or_else(|| {
        invalid_input_error(
            "host keyring secret reference must follow keyring://<service>/<account>",
        )
    })?;

    Ok((
        require_non_empty(service, "host keyring service")?,
        require_non_empty(account, "host keyring account")?,
    ))
}

fn open_host_keyring_entry(service: &str, account: &str) -> io::Result<Entry> {
    Entry::new(service, account).map_err(keyring_error)
}

fn keyring_error(error: keyring::Error) -> io::Error {
    io::Error::other(format!("host keyring error: {error}"))
}

fn validate_optional_credentials_binding(
    connection: &Connection,
    credentials_id: Option<i64>,
) -> io::Result<()> {
    let Some(credentials_id) = credentials_id else {
        return Ok(());
    };

    require_positive_identifier(credentials_id, "credentials id")?;
    let exists = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM credentials WHERE id = ?)",
            [credentials_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(sqlite_error)?;
    if exists == 0 {
        return Err(not_found_error(format!(
            "credentials {credentials_id} were not found"
        )));
    }

    Ok(())
}

fn duration_to_millis(duration: Duration, label: &str) -> io::Result<i64> {
    if duration.is_zero() {
        return Err(invalid_input_error(format!(
            "{label} must be greater than zero"
        )));
    }

    i64::try_from(duration.as_millis())
        .map_err(|error| io::Error::new(ErrorKind::InvalidInput, error))
}

fn unix_timestamp_millis() -> io::Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;

    i64::try_from(duration.as_millis())
        .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))
}

/// Persists one shell-to-runtime control request for the next runtime loop.
pub fn enqueue_runtime_control_request(
    storage: &StorageLayout,
    request: &RuntimeControlRequest,
) -> io::Result<PathBuf> {
    fs::create_dir_all(&storage.runtime_control_requests_dir)?;

    let request_path = storage
        .runtime_control_requests_dir
        .join(format!("{}.json", next_token("runtime-control")?));
    let content = serde_json::to_vec_pretty(request).map_err(io::Error::other)?;

    fs::write(&request_path, content)?;
    Ok(request_path)
}

/// Loads and removes the queued shell-to-runtime control requests.
pub fn take_runtime_control_requests(
    storage: &StorageLayout,
) -> io::Result<Vec<RuntimeControlRequest>> {
    let entries = match fs::read_dir(&storage.runtime_control_requests_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut request_paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.is_file() {
            request_paths.push(path);
        }
    }
    request_paths.sort();

    let mut requests = Vec::with_capacity(request_paths.len());
    for request_path in request_paths {
        let content = fs::read(&request_path)?;
        let request: RuntimeControlRequest =
            serde_json::from_slice(&content).map_err(|error| {
                io::Error::new(
                    ErrorKind::InvalidData,
                    format!(
                        "failed to decode runtime control request {}: {error}",
                        request_path.display(),
                    ),
                )
            })?;

        fs::remove_file(&request_path)?;
        requests.push(request);
    }

    Ok(requests)
}

fn next_poll_interval(wait: Duration, elapsed: Duration) -> Duration {
    wait.saturating_sub(elapsed)
        .min(Duration::from_millis(COORDINATION_POLL_INTERVAL_MILLIS))
}

fn next_token(prefix: &str) -> io::Result<String> {
    let issued_at_unix_millis = unix_timestamp_millis()?;
    let sequence = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);

    Ok(format!(
        "{prefix}-{}-{issued_at_unix_millis}-{sequence}",
        std::process::id()
    ))
}

fn sqlite_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(error)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_pragmas, decode_release_dispatch_job,
        ensure_migration_ledger, initialize_database,
        list_artifact_inspection_records, list_build_history_records,
        list_process_feed_page, list_process_feed_page_filtered,
        list_publish_target_binding_runtime_settings,
        list_build_target_runtime_settings,
        resolve_build_target_read_model_projection,
        CreateRepositoryProjectBuildTargetInput,
        CreateRepositoryProjectCredentialInput,
        CreateRepositoryProjectInput,
        ProcessFeedPageQuery, ProcessFeedScope, ProcessFeedStatusFilter,
        UpdateRepositoryAuthStateInput,
        UpdateRepositoryProjectBuildTargetInput,
        UpdateRepositoryProjectInput,
        list_credential_records,
        list_publish_target_runtime_settings, open_connection, recover_runtime_state,
        select_orphan_build_process_roots,
        BuildDispatchJob, BuildExecutionPlan, BuildRunRecord,
        BuildTargetRuntimeSettingsRecord, CancelBuildRunInput, CancelReleaseRunInput,
        CreatedRepositoryProjectRecord,
        CompleteBuildRunInput, CreateArtifactRecordInput, FailBuildRunInput,
        CompletePublishRunInput, CredentialRecord, LocalCoordinator,
        InterruptedBuildRecoveryRecord, ObservedProcess,
        ManualReleaseDispatchInput, PublishDispatchJob, QueueDispatchOutcome,
        PublishTargetRuntimeSettingsRecord, ReleaseDispatchJob,
        ReleaseRunRecord, RuntimeRecoveryReport,
        RuntimeControlRequest,
        StartBuildRunInput, StartPublishRunInput, StorageLayout,
        enqueue_runtime_control_request,
        take_runtime_control_requests,
        UpsertCredentialRecordInput,
        KIND_GIT_HTTP_BASIC,
        RECOVERY_INTERRUPTION_KIND_SYSTEM,
        DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        MIGRATIONS,
        PROJECT_VERSION_FILE_PATH, TRIGGER_SOURCE_MANUAL,
        TRIGGER_SOURCE_POLL,
        BUILD_RUN_QUEUE_NAME, PUBLISH_RUN_QUEUE_NAME, RELEASE_RUN_QUEUE_NAME,
        SQLITE_BUSY_TIMEOUT_MILLIS,
    };
    use runtime_contracts::{BuildKind, EngineKind};
    use crate::lifecycle::{BuildStatus, PublishStatus, ReleaseStatus};
    use runtime_config::{RuntimeConcurrencySettings, RuntimeDirectories};
    use rusqlite::{params, Connection};
    use serde_json::Value;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    use crate::release_dispatch_idempotency_key;

    #[test]
    fn storage_layout_uses_state_and_logs_directories() {
        let directories = RuntimeDirectories::from_root(PathBuf::from("/tmp/runtime"));
        let layout = StorageLayout::from_directories(&directories);

        assert_eq!(layout.database_path, PathBuf::from("/tmp/runtime/state/runtime.db"));
        assert_eq!(layout.health_report_path, PathBuf::from("/tmp/runtime/state/health.json"));
        assert_eq!(
            layout.automation_state_path,
            PathBuf::from("/tmp/runtime/state/automation-state.json")
        );
        assert_eq!(
            layout.supervision_contract_path,
            PathBuf::from("/tmp/runtime/state/supervision.json")
        );
        assert_eq!(
            layout.supervisor_state_path,
            PathBuf::from("/tmp/runtime/state/supervisor-state.json")
        );
        assert_eq!(
            layout.runtime_events_path,
            PathBuf::from("/tmp/runtime/state/runtime-events.jsonl")
        );
        assert_eq!(
            layout.runtime_events_cursor_path,
            PathBuf::from("/tmp/runtime/state/runtime-events.cursor.json")
        );
        assert_eq!(
            layout.runtime_control_requests_dir,
            PathBuf::from("/tmp/runtime/state/runtime-control")
        );
        assert_eq!(
            layout.runtime_log_path,
            PathBuf::from("/tmp/runtime/logs/runtime.jsonl")
        );
    }

    #[test]
    fn runtime_control_requests_round_trip_through_request_directory() {
        let root = std::env::temp_dir().join("runtime-store-control-requests-test");
        if root.exists() {
            std::fs::remove_dir_all(&root)
                .expect("existing runtime control test root should be removable");
        }

        let directories = RuntimeDirectories::from_root(&root);
        let storage = StorageLayout::from_directories(&directories);

        enqueue_runtime_control_request(
            &storage,
            &RuntimeControlRequest::ForceRepositoryPoll { repository_id: 7 },
        )
        .expect("first runtime control request should queue");
        enqueue_runtime_control_request(
            &storage,
            &RuntimeControlRequest::ForceRepositoryPoll { repository_id: 11 },
        )
        .expect("second runtime control request should queue");

        let requests = take_runtime_control_requests(&storage)
            .expect("runtime control requests should load");

        assert_eq!(
            requests,
            vec![
                RuntimeControlRequest::ForceRepositoryPoll { repository_id: 7 },
                RuntimeControlRequest::ForceRepositoryPoll { repository_id: 11 },
            ]
        );
        assert!(
            std::fs::read_dir(&storage.runtime_control_requests_dir)
                .expect("runtime control request directory should still exist")
                .next()
                .is_none()
        );

        std::fs::remove_dir_all(&root)
            .expect("runtime control test root should be removable");
    }

    #[test]
    fn initialize_database_applies_pragmas_and_migrations() {
        let root = test_root("bootstrap");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);

        let report = initialize_database(&layout).expect("database bootstrap should succeed");

        assert!(report.database_path.exists());
        assert_eq!(report.busy_timeout_millis, SQLITE_BUSY_TIMEOUT_MILLIS);
        assert!(report.foreign_keys_enabled);
        assert_eq!(report.journal_mode, "wal");
        assert_eq!(
            report.applied_migrations,
            vec![
                "0001_runtime_metadata.sql",
                "0002_pipeline_definitions.sql",
                "0003_execution_runs.sql",
                "0004_local_coordination.sql",
                "0005_host_native_runner_defaults.sql",
                "0006_repository_source_configuration.sql",
                "0007_repository_path_model_cleanup.sql",
                "0008_build_run_stage_tracking.sql",
                "0009_build_target_runner_model_cleanup.sql",
                "0009_execution_retention.sql",
                "0010_engine_contract_model.sql",
                "0011_runtime_engine_version.sql",
                "0012_repository_auth_state.sql",
                "0013_publish_destination_execution_contract.sql",
                "0014_release_source_identity.sql",
                "0015_local_workspace_polling_invariant.sql",
            ]
        );

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert!(table_exists(&connection, "schema_migrations"));
        assert!(table_exists(&connection, "runtime_metadata"));
        assert!(table_exists(&connection, "app_settings"));
        assert!(table_exists(&connection, "credentials"));
        assert!(table_exists(&connection, "repositories"));
        assert!(table_exists(&connection, "trigger_rules"));
        assert!(table_exists(&connection, "build_targets"));
        assert!(table_exists(&connection, "publish_targets"));
        assert!(table_exists(&connection, "build_publish_bindings"));
        assert!(table_exists(&connection, "release_runs"));
        assert!(table_exists(&connection, "build_runs"));
        assert!(table_exists(&connection, "artifacts"));
        assert!(table_exists(&connection, "publish_runs"));
        assert!(table_exists(&connection, "build_run_steps"));
        assert!(table_exists(&connection, "worker_queue_messages"));
        assert!(table_exists(&connection, "worker_coordination_leases"));
        assert!(table_exists(&connection, "worker_idempotency_keys"));
        assert!(table_has_columns(
            &connection,
            "repositories",
            &[
                "name",
                "source_mode",
                "workspace_strategy",
                "repo_url",
                "local_path",
                "credentials_id",
                "default_branch",
                "artifacts_root_override",
                "workspace_root_override",
                "polling_interval_seconds",
                "last_seen_tag",
                "enabled"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "build_targets",
            &[
                "repository_id",
                "build_kind",
                "runner_type",
                "output_path_template",
                "timeout_seconds",
                "contract_json",
                "config_json"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "release_runs",
            &[
                "repository_id",
                "git_tag",
                "git_commit",
                "trigger_source",
                "trigger_rule_id",
                "source_metadata_json",
                "engine_version",
                "status"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "build_runs",
            &[
                "release_run_id",
                "build_target_id",
                "engine_version",
                "image_ref",
                "workspace_path",
                "log_path",
                "artifact_root_path",
                "status"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "publish_runs",
            &[
                "release_run_id",
                "build_run_id",
                "publish_target_id",
                "artifact_id",
                "destination_ref",
                "status"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "worker_queue_messages",
            &[
                "queue_name",
                "payload",
                "leased_by",
                "lease_token",
                "lease_expires_at_unix_millis",
                "dequeue_count"
            ]
        ));
        assert!(table_has_columns(
            &connection,
            "worker_coordination_leases",
            &["name", "token", "lease_expires_at_unix_millis"]
        ));
        assert!(table_has_columns(
            &connection,
            "worker_idempotency_keys",
            &["idempotency_key", "claim_expires_at_unix_millis"]
        ));
        assert!(index_exists(&connection, "idx_build_targets_repository_id"));
        assert!(index_exists(&connection, "idx_publish_targets_repository_id"));
        assert!(index_exists(&connection, "idx_trigger_rules_repository_source"));
        assert!(index_exists(&connection, "idx_release_runs_repository_status"));
        assert!(index_exists(&connection, "idx_release_runs_trigger_source_status"));
        assert!(index_exists(&connection, "idx_build_runs_release_status"));
        assert!(index_exists(&connection, "idx_artifacts_build_run_id"));
        assert!(index_exists(&connection, "idx_publish_runs_release_status"));
        assert!(index_exists(&connection, "idx_worker_queue_messages_claim"));
        assert!(index_exists(&connection, "idx_worker_coordination_leases_expiry"));
        assert!(index_exists(&connection, "idx_worker_idempotency_keys_expiry"));
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn create_repository_project_persists_repository_credentials_and_targets() {
        let root = test_root("create-repository-project");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let created = LocalCoordinator::new(&layout)
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Red Horizon"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/red-horizon.git")),
                local_path: None,
                credentials: Some(CreateRepositoryProjectCredentialInput {
                    name: String::from("Red Horizon/origin"),
                    kind: String::from(KIND_GIT_HTTP_BASIC),
                    config_json: String::from(
                        r#"{"username":"git","password":"keyring://handy-games-publisher/credential/red-horizon/origin"}"#,
                    ),
                }),
                default_branch: Some(String::from("main")),
                artifacts_root_override: Some(String::from("C:/builds/red-horizon")),
                workspace_root_override: Some(String::from("C:/workspaces/red-horizon")),
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![
                    CreateRepositoryProjectBuildTargetInput {
                        name: String::from("Windows"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "StandaloneWindows64",
                                "buildMethod": "Builder.PerformWindows"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                    CreateRepositoryProjectBuildTargetInput {
                        name: String::from("WebGL"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "WebGL",
                                "buildMethod": "Builder.PerformWebGL"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                ],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");

        assert_eq!(
            created,
            CreatedRepositoryProjectRecord {
                repository_id: created.repository_id,
                repository_name: String::from("Red Horizon"),
                credentials_id: created.credentials_id,
                build_target_ids: created.build_target_ids.clone(),
            }
        );
        assert_eq!(created.build_target_ids.len(), 2);
        assert!(created.credentials_id.is_some());

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repository_row = connection
            .query_row(
                "
                SELECT source_mode,
                       workspace_strategy,
                       repo_url,
                       default_branch,
                       artifacts_root_override,
                       workspace_root_override,
                       polling_interval_seconds,
                      engine_kind,
                       enabled,
                       credentials_id
                FROM repositories
                WHERE id = ?
                ",
                [created.repository_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, Option<i64>>(9)?,
                    ))
                },
            )
            .expect("repository row should exist");
        assert_eq!(repository_row.0, "managed_repository");
        assert_eq!(repository_row.1, "managed_checkout");
        assert_eq!(repository_row.2, "https://example.com/red-horizon.git");
        assert_eq!(repository_row.3.as_deref(), Some("main"));
        assert_eq!(repository_row.4.as_deref(), Some("C:/builds/red-horizon"));
        assert_eq!(repository_row.5.as_deref(), Some("C:/workspaces/red-horizon"));
        assert_eq!(repository_row.6, 300);
        assert_eq!(repository_row.7, "unity");
        assert_eq!(repository_row.8, 1);
        assert_eq!(repository_row.9, created.credentials_id);

        let credential_row = connection
            .query_row(
                "SELECT name, kind, config_json FROM credentials WHERE id = ?",
                [created.credentials_id.expect("credentials id should exist")],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .expect("credential row should exist");
        assert_eq!(credential_row.0, "Red Horizon/origin");
        assert_eq!(credential_row.1, KIND_GIT_HTTP_BASIC);
        assert!(credential_row.2.contains("keyring://handy-games-publisher/"));

        let trigger_rule_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM trigger_rules WHERE repository_id = ? AND source = ?",
                params![created.repository_id, TRIGGER_SOURCE_POLL],
                |row| row.get(0),
            )
            .expect("poll trigger rule count should load");
        assert_eq!(trigger_rule_count, 1);

        let persisted_targets: Vec<(String, String, String)> = {
            let mut statement = connection
                .prepare(
                    "
                    SELECT name,
                           build_kind,
                           contract_json
                    FROM build_targets
                    WHERE repository_id = ?
                    ORDER BY name ASC
                    ",
                )
                .expect("build targets query should prepare");
            let rows = statement
                .query_map([created.repository_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .expect("build target rows should map");
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .expect("build target rows should collect")
        };
        assert_eq!(persisted_targets.len(), 2);
        assert!(persisted_targets.iter().all(|target| target.1 == "player"));
        assert_eq!(persisted_targets[0].0, "WebGL");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&persisted_targets[0].2)
                .expect("contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "WebGL",
                    "buildMethod": "Builder.PerformWebGL"
                }
            })
        );
        assert_eq!(persisted_targets[1].0, "Windows");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&persisted_targets[1].2)
                .expect("contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "StandaloneWindows64",
                    "buildMethod": "Builder.PerformWindows"
                }
            })
        );

        let build_targets = list_build_target_runtime_settings(&layout)
            .expect("build targets should load");
        assert_eq!(build_targets.len(), 2);
        assert!(build_targets.iter().all(|target| target.repository_id == created.repository_id));
        assert!(build_targets.iter().all(|target| target.runner_type == DEFAULT_HOST_NATIVE_RUNNER_TYPE));
        assert!(build_targets.iter().all(|target| target.config_json.contains("unity_executable_path")));
    }

    #[test]
    fn update_repository_project_persists_repository_configuration_changes() {
        let root = test_root("update-repository-project");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let created = LocalCoordinator::new(&layout)
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Old Banner"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/old-banner.git")),
                local_path: None,
                credentials: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: Some(String::from("C:/artifacts/old-banner")),
                workspace_root_override: Some(String::from("C:/workspaces/old-banner")),
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![CreateRepositoryProjectBuildTargetInput {
                    name: String::from("Windows"),
                    build_kind: String::from("player"),
                    runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: 3600,
                    enabled: true,
                    contract_json: serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.PerformWindows"
                        }
                    })
                    .to_string(),
                    runner_config_json: String::from(
                        r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                    ),
                }],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");

        LocalCoordinator::new(&layout)
            .update_repository_project(UpdateRepositoryProjectInput {
                repository_id: created.repository_id,
                name: String::from("New Banner"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/new-banner.git")),
                local_path: None,
                default_branch: Some(String::from("release")),
                artifacts_root_override: None,
                workspace_root_override: Some(String::from("D:/workspaces/new-banner")),
                polling_interval_seconds: 30,
                enabled: false,
                build_targets: vec![UpdateRepositoryProjectBuildTargetInput {
                    build_target_id: Some(created.build_target_ids[0]),
                    name: String::from("Windows"),
                    build_kind: String::from("player"),
                    runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: 3600,
                    enabled: true,
                    contract_json: serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.PerformWindowsStable",
                            "editorVersion": "2022.3.14f1"
                        }
                    })
                    .to_string(),
                    runner_config_json: String::from(
                        r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                    ),
                }],
                publish_targets: Vec::new(),
            })
            .expect("repository project should update");

        let repositories = LocalCoordinator::new(&layout)
            .list_polling_repositories()
            .expect("managed polling repositories should load");

        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].id, created.repository_id);
        assert_eq!(repositories[0].name, "New Banner");
        assert_eq!(repositories[0].repo_url, "https://example.com/new-banner.git");
        assert_eq!(repositories[0].default_branch.as_deref(), Some("release"));
        assert_eq!(repositories[0].artifacts_root_override, None);
        assert_eq!(
            repositories[0].workspace_root_override.as_deref(),
            Some("D:/workspaces/new-banner")
        );
        assert_eq!(repositories[0].polling_interval_seconds, 30);
        assert!(!repositories[0].enabled);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repository_row = connection
            .query_row(
                "
                SELECT name,
                      engine_kind,
                       repo_url,
                       default_branch,
                       artifacts_root_override,
                       workspace_root_override,
                       polling_interval_seconds,
                       enabled
                FROM repositories
                WHERE id = ?
                ",
                [created.repository_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                },
            )
            .expect("repository row should reload");
        assert_eq!(repository_row.0, "New Banner");
        assert_eq!(repository_row.1, "unity");
        assert_eq!(repository_row.2, "https://example.com/new-banner.git");
        assert_eq!(repository_row.3.as_deref(), Some("release"));
        assert_eq!(repository_row.4, None);
        assert_eq!(repository_row.5.as_deref(), Some("D:/workspaces/new-banner"));
        assert_eq!(repository_row.6, 30);
        assert_eq!(repository_row.7, 0);

        let build_target_row: (String, String, String) =
            connection
                .query_row(
                    "
                    SELECT build_kind,
                           contract_json,
                           config_json
                    FROM build_targets
                    WHERE id = ?
                    ",
                    [created.build_target_ids[0]],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .expect("updated build target should reload");
        assert_eq!(build_target_row.0, "player");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&build_target_row.1)
                .expect("contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "StandaloneWindows64",
                    "buildMethod": "Builder.PerformWindowsStable",
                    "editorVersion": "2022.3.14f1"
                }
            })
        );
        assert_eq!(
            build_target_row.2,
            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#
        );

        drop(connection);
    }

    #[test]
    fn repository_auth_state_tracks_binding_transitions() {
        let root = test_root("repository-auth-state-binding-transitions");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let created = LocalCoordinator::new(&layout)
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Night Shift"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://github.com/indiegabo/night-shift.git")),
                local_path: None,
                credentials: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![CreateRepositoryProjectBuildTargetInput {
                    name: String::from("Windows"),
                    build_kind: String::from("player"),
                    runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: 3600,
                    enabled: true,
                    contract_json: serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.PerformWindows"
                        }
                    })
                    .to_string(),
                    runner_config_json: String::from(
                        r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                    ),
                }],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .update_repository_auth_state(UpdateRepositoryAuthStateInput {
                repository_id: created.repository_id,
                source_provider_id: String::from("github"),
                source_instance_url: String::from("https://github.com"),
                visibility_status: String::from("private"),
                auth_requirement_status: String::from("required"),
                supports_interactive_login: true,
                auth_status_message: String::from(
                    "GitHub repository requires authentication before HGP can access it.",
                ),
            })
            .expect("repository auth state should persist");

        let unbound = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load");
        assert_eq!(unbound.len(), 1);
        assert_eq!(unbound[0].auth_binding_status, "required_unbound");
        assert_eq!(unbound[0].visibility_status, "private");
        assert_eq!(unbound[0].auth_requirement_status, "required");
        assert_eq!(unbound[0].source_provider_id.as_deref(), Some("github"));
        assert!(unbound[0].auth_last_verified_at.is_some());

        let connection = open_connection(&layout.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "GitHub.com",
                    KIND_GIT_HTTP_BASIC,
                    r#"{"username":"git","password":"secret"}"#,
                ],
            )
            .expect("credential row should insert");
        let credentials_id = connection.last_insert_rowid();
        drop(connection);

        coordinator
            .update_repository_credentials_binding(
                created.repository_id,
                Some(credentials_id),
            )
            .expect("repository credentials should bind");

        let bound = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load after binding");
        assert_eq!(bound[0].auth_binding_status, "bound_ready");
        assert_eq!(bound[0].credentials_id, Some(credentials_id));

        coordinator
            .update_repository_auth_state(UpdateRepositoryAuthStateInput {
                repository_id: created.repository_id,
                source_provider_id: String::from("github"),
                source_instance_url: String::from("https://github.com"),
                visibility_status: String::from("public"),
                auth_requirement_status: String::from("none"),
                supports_interactive_login: true,
                auth_status_message: String::from(
                    "Public repository detected through anonymous remote access.",
                ),
            })
            .expect("public repository auth state should clear incompatible binding");

        let public = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load after public recheck");
        assert_eq!(public[0].auth_binding_status, "not_required");
        assert_eq!(public[0].credentials_id, None);
        assert_eq!(public[0].visibility_status, "public");

        coordinator
            .update_repository_auth_state(UpdateRepositoryAuthStateInput {
                repository_id: created.repository_id,
                source_provider_id: String::from("github"),
                source_instance_url: String::from("https://github.com"),
                visibility_status: String::from("private"),
                auth_requirement_status: String::from("required"),
                supports_interactive_login: true,
                auth_status_message: String::from(
                    "GitHub repository requires authentication before HGP can access it.",
                ),
            })
            .expect("private repository auth state should re-enter required binding flow");

        coordinator
            .update_repository_credentials_binding(
                created.repository_id,
                Some(credentials_id),
            )
            .expect("repository credentials should rebind after private recheck");

        coordinator
            .update_repository_credentials_binding(created.repository_id, None)
            .expect("repository credentials should clear");

        let cleared = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load after clearing");
        assert_eq!(cleared[0].auth_binding_status, "required_unbound");
        assert_eq!(cleared[0].credentials_id, None);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn repository_auth_runtime_failure_marks_reauth_required_for_bound_private_repository() {
        let root = test_root("repository-auth-runtime-failure");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let created = LocalCoordinator::new(&layout)
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Night Shift"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://github.com/indiegabo/night-shift.git")),
                local_path: None,
                credentials: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![CreateRepositoryProjectBuildTargetInput {
                    name: String::from("Windows"),
                    build_kind: String::from("player"),
                    runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                    output_kind: Some(String::from("archive")),
                    output_path_template: None,
                    timeout_seconds: 3600,
                    enabled: true,
                    contract_json: serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.PerformWindows"
                        }
                    })
                    .to_string(),
                    runner_config_json: String::from(
                        r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                    ),
                }],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .update_repository_auth_state(UpdateRepositoryAuthStateInput {
                repository_id: created.repository_id,
                source_provider_id: String::from("github"),
                source_instance_url: String::from("https://github.com"),
                visibility_status: String::from("private"),
                auth_requirement_status: String::from("required"),
                supports_interactive_login: true,
                auth_status_message: String::from(
                    "GitHub repository requires authentication before HGP can access it.",
                ),
            })
            .expect("repository auth state should persist");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "GitHub.com",
                    KIND_GIT_HTTP_BASIC,
                    r#"{"username":"git","password":"secret"}"#,
                ],
            )
            .expect("credential row should insert");
        let credentials_id = connection.last_insert_rowid();
        drop(connection);

        coordinator
            .update_repository_credentials_binding(
                created.repository_id,
                Some(credentials_id),
            )
            .expect("repository credentials should bind");

        coordinator
            .mark_repository_auth_runtime_failure(
                created.repository_id,
                "Authentication failed for https://github.com/indiegabo/night-shift.git",
            )
            .expect("runtime auth failure should persist");

        let repositories = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load");
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].auth_binding_status, "reauth_required");
        assert_eq!(repositories[0].credentials_id, Some(credentials_id));
        assert!(repositories[0]
            .auth_status_message
            .contains("Authentication failed"));
        assert!(repositories[0].auth_last_verified_at.is_some());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn update_repository_project_syncs_active_build_targets_without_deleting_history() {
        let root = test_root("update-repository-project-build-target-sync");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let created = LocalCoordinator::new(&layout)
            .create_repository_project(CreateRepositoryProjectInput {
                name: String::from("Target Sync"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/target-sync.git")),
                local_path: None,
                credentials: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![
                    CreateRepositoryProjectBuildTargetInput {
                        name: String::from("Windows"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "StandaloneWindows64",
                                "buildMethod": "Builder.PerformWindows",
                                "editorVersion": "2022.3.14f1"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                    CreateRepositoryProjectBuildTargetInput {
                        name: String::from("Linux"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "StandaloneLinux64",
                                "buildMethod": "Builder.PerformLinux",
                                "editorVersion": "2022.3.14f1"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                ],
                publish_targets: Vec::new(),
            })
            .expect("repository project should persist");

        let linux_target_id = created.build_target_ids[1];
        let connection = open_connection(&layout.database_path).expect("connection should open");
        let release_run_id = insert_release_run(
            &connection,
            created.repository_id,
            "v1.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            linux_target_id,
            BuildStatus::Queued.as_str(),
        );
        drop(connection);

        LocalCoordinator::new(&layout)
            .update_repository_project(UpdateRepositoryProjectInput {
                repository_id: created.repository_id,
                name: String::from("Target Sync"),
                engine_kind: String::from("unity"),
                source_mode: String::from("managed_repository"),
                repo_url: Some(String::from("https://example.com/target-sync.git")),
                local_path: None,
                default_branch: Some(String::from("main")),
                artifacts_root_override: None,
                workspace_root_override: None,
                polling_interval_seconds: 300,
                enabled: true,
                build_targets: vec![
                    UpdateRepositoryProjectBuildTargetInput {
                        build_target_id: Some(created.build_target_ids[0]),
                        name: String::from("Windows Stable"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "StandaloneWindows64",
                                "buildMethod": "Builder.PerformWindowsStable",
                                "editorVersion": "2022.3.14f1"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                    UpdateRepositoryProjectBuildTargetInput {
                        build_target_id: None,
                        name: String::from("WebGL"),
                        build_kind: String::from("player"),
                        runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                        output_kind: Some(String::from("archive")),
                        output_path_template: None,
                        timeout_seconds: 3600,
                        enabled: true,
                        contract_json: serde_json::json!({
                            "unity": {
                                "targetPlatform": "WebGL",
                                "buildMethod": "Builder.PerformWebGl"
                            }
                        })
                        .to_string(),
                        runner_config_json: String::from(
                            r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                        ),
                    },
                ],
                publish_targets: Vec::new(),
            })
            .expect("repository project should sync build targets");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let build_run_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE id = ?",
                [build_run_id],
                |row| row.get(0),
            )
            .expect("build run count should load");
        assert_eq!(build_run_count, 1);

        let target_rows = {
            let mut statement = connection
                .prepare(
                    "
                    SELECT id, name, contract_json, enabled
                    FROM build_targets
                    WHERE repository_id = ?
                    ORDER BY id
                    ",
                )
                .expect("build targets query should prepare");
            let rows = statement
                .query_map([created.repository_id], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                })
                .expect("build targets query should execute");
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .expect("build targets should collect")
        };

        assert_eq!(target_rows.len(), 3);
        assert_eq!(target_rows[0].0, created.build_target_ids[0]);
        assert_eq!(target_rows[0].1, "Windows Stable");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&target_rows[0].2)
                .expect("target contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "StandaloneWindows64",
                    "buildMethod": "Builder.PerformWindowsStable",
                    "editorVersion": "2022.3.14f1"
                }
            })
        );
        assert_eq!(target_rows[0].3, 1);

        assert_eq!(target_rows[1].0, linux_target_id);
        assert_eq!(target_rows[1].1, "Linux");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&target_rows[1].2)
                .expect("target contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "StandaloneLinux64",
                    "buildMethod": "Builder.PerformLinux",
                    "editorVersion": "2022.3.14f1"
                }
            })
        );
        assert_eq!(target_rows[1].3, 0);

        assert_eq!(target_rows[2].1, "WebGL");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&target_rows[2].2)
                .expect("target contract_json should decode"),
            serde_json::json!({
                "unity": {
                    "targetPlatform": "WebGL",
                    "buildMethod": "Builder.PerformWebGl"
                }
            })
        );
        assert_eq!(target_rows[2].3, 1);

        let enabled_target_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM build_targets WHERE repository_id = ? AND enabled = 1",
                [created.repository_id],
                |row| row.get(0),
            )
            .expect("enabled target count should load");
        assert_eq!(enabled_target_count, 2);

        drop(connection);
        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn resolve_build_target_read_model_projection_prefers_contract_values() {
        let projection = resolve_build_target_read_model_projection(
            "unity",
            "player",
            &serde_json::json!({
                "unity": {
                    "targetPlatform": "StandaloneWindows64",
                    "buildMethod": "Builder.PerformWindows",
                    "editorVersion": ""
                }
            })
            .to_string(),
        )
        .expect("contract projection should load");

        assert_eq!(projection.unity_target_platform, "StandaloneWindows64");
        assert_eq!(
            projection.unity_build_method.as_deref(),
            Some("Builder.PerformWindows")
        );
    }

    #[test]
    fn resolve_build_target_read_model_projection_rejects_missing_contract_values() {
        let error = resolve_build_target_read_model_projection(
            "unity",
            "player",
            "{}",
        )
        .expect_err("read model projection should require a Unity contract");

        assert!(
            error
                .to_string()
                .contains("contract_json must define contract.unity")
        );
    }

    #[test]
    fn list_build_target_runtime_settings_returns_persisted_target_config() {
        let root = test_root("list-build-target-runtime-settings");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "runtime-settings");
        connection
            .execute(
                "
                UPDATE build_targets
                SET contract_json = ?,
                    config_json = ?
                WHERE id = ?
                ",
                params![
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "CI.Build.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                    r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                    fixture.primary_build_target_id,
                ],
            )
            .expect("primary build target config should update");
        connection
            .execute(
                "UPDATE build_targets SET enabled = 0 WHERE id = ?",
                [fixture.secondary_build_target_id],
            )
            .expect("secondary build target should disable");
        drop(connection);

        let targets = list_build_target_runtime_settings(&layout)
            .expect("build target settings should load");

        assert_eq!(targets.len(), 2);
        assert_eq!(
            targets[0],
            BuildTargetRuntimeSettingsRecord {
                id: fixture.primary_build_target_id,
                repository_id: fixture.repository_id,
                repository_name: String::from("runtime-settings"),
                name: String::from("runtime-settings-windows"),
                unity_target_platform: String::from("StandaloneWindows64"),
                runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                unity_build_method: Some(String::from("CI.Build.Perform")),
                enabled: true,
                config_json: String::from(
                    r#"{"unity_executable_path":"C:/Unity/Editor/Unity.exe"}"#,
                ),
            }
        );
        assert_eq!(targets[1].id, fixture.secondary_build_target_id);
        assert_eq!(targets[1].repository_id, fixture.repository_id);
        assert_eq!(targets[1].repository_name, "runtime-settings");
        assert_eq!(targets[1].name, "runtime-settings-linux");
        assert_eq!(targets[1].unity_target_platform, "StandaloneLinux64");
        assert_eq!(targets[1].runner_type, DEFAULT_HOST_NATIVE_RUNNER_TYPE);
        assert_eq!(
            targets[1].unity_build_method.as_deref(),
            Some("Builder.Perform")
        );
        assert!(!targets[1].enabled);
        assert_eq!(targets[1].config_json, "{}");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_build_history_records_returns_joined_build_activity() {
        let root = test_root("list-build-history-records");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "build-history");
        let release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v4.0.0",
            ReleaseStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    engine_version = ?
                WHERE id = ?
                ",
                params!["cafebabe", "2022.3.20f1", release_run_id],
            )
            .expect("release metadata should update");
        connection
            .execute(
                "
                UPDATE build_targets
                SET contract_json = ?,
                    runner_type = ?
                WHERE id = ?
                ",
                params![
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "CI.Build.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                    DEFAULT_HOST_NATIVE_RUNNER_TYPE,
                    fixture.primary_build_target_id,
                ],
            )
            .expect("build target metadata should update");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        update_build_run_plan(
            &connection,
            build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    log_path = ?,
                    artifact_root_path = ?,
                    started_at = ?,
                    finished_at = ?
                WHERE id = ?
                ",
                params![
                    "C:/runs/build-history",
                    "C:/logs/build-history.log",
                    "C:/artifacts/build-history",
                    "2026-01-10T10:00:00Z",
                    "2026-01-10T10:05:00Z",
                    build_run_id,
                ],
            )
            .expect("build run metadata should update");
        let artifact_id = insert_artifact(&connection, build_run_id, "history.zip");
        insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            fixture.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let records = list_build_history_records(&layout)
            .expect("build history should load joined build activity");

        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.build_run_id, build_run_id);
        assert_eq!(record.release_run_id, release_run_id);
        assert_eq!(record.repository_id, fixture.repository_id);
        assert_eq!(record.repository_name, "build-history");
        assert_eq!(
            record.repository_url.as_deref(),
            Some("https://example.com/build-history.git")
        );
        assert_eq!(record.git_tag, "v4.0.0");
        assert_eq!(record.git_commit.as_deref(), Some("cafebabe"));
        assert_eq!(record.build_target_id, fixture.primary_build_target_id);
        assert_eq!(record.build_target_name, "build-history-windows");
        assert_eq!(record.unity_target_platform, "StandaloneWindows64");
        assert_eq!(record.runner_type, DEFAULT_HOST_NATIVE_RUNNER_TYPE);
        assert_eq!(
            record.unity_build_method.as_deref(),
            Some("CI.Build.Perform")
        );
        assert_eq!(record.engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(record.image_ref.as_deref(), Some(DEFAULT_HOST_NATIVE_RUNNER_TYPE));
        assert_eq!(record.status, BuildStatus::Succeeded.as_str());
        assert_eq!(
            record.workspace_path.as_deref(),
            Some("C:/runs/build-history")
        );
        assert_eq!(record.log_path.as_deref(), Some("C:/logs/build-history.log"));
        assert_eq!(
            record.artifact_root_path.as_deref(),
            Some("C:/artifacts/build-history")
        );
        assert_eq!(record.started_at.as_deref(), Some("2026-01-10T10:00:00Z"));
        assert_eq!(record.finished_at.as_deref(), Some("2026-01-10T10:05:00Z"));
        assert!(record.error_message.is_none());
        assert_eq!(record.artifact_count, 1);
        assert_eq!(record.publish_run_count, 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_process_feed_page_returns_release_level_history_and_pagination() {
        let root = test_root("list-process-feed-page");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "process-feed");

        let completed_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.0.0",
            ReleaseStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    engine_version = ?,
                    started_at = ?,
                    finished_at = ?
                WHERE id = ?
                ",
                params![
                    "cafebabe",
                    "2022.3.18f1",
                    "2026-01-10T10:00:00Z",
                    "2026-01-10T10:04:00Z",
                    completed_release_run_id,
                ],
            )
            .expect("completed release metadata should update");
        let completed_build_run_id = insert_build_run(
            &connection,
            completed_release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        update_build_run_plan(
            &connection,
            completed_build_run_id,
            "2022.3.18f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET started_at = ?,
                    finished_at = ?
                WHERE id = ?
                ",
                params![
                    "2026-01-10T10:01:00Z",
                    "2026-01-10T10:03:00Z",
                    completed_build_run_id,
                ],
            )
            .expect("completed build metadata should update");

        let running_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.1.0",
            ReleaseStatus::Queued.as_str(),
        );
        connection
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    engine_version = ?,
                    started_at = ?
                WHERE id = ?
                ",
                params![
                    "deadbeef",
                    "2022.3.20f1",
                    "2026-01-11T11:00:00Z",
                    running_release_run_id,
                ],
            )
            .expect("running release metadata should update");
        let running_build_run_id = insert_build_run(
            &connection,
            running_release_run_id,
            fixture.secondary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        update_build_run_plan(
            &connection,
            running_build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET current_stage_key = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    heartbeat_at = ?,
                    last_progress_message = ?,
                    started_at = ?
                WHERE id = ?
                ",
                params![
                    "build-player",
                    "Build player",
                    BuildStatus::Running.as_str(),
                    "2026-01-11T11:02:00Z",
                    "Packaging Linux player",
                    "2026-01-11T11:01:00Z",
                    running_build_run_id,
                ],
            )
            .expect("running build metadata should update");
        drop(connection);

        let first_page = list_process_feed_page(&layout, 1, 1)
            .expect("first process feed page should load");

        assert!(!first_page.generated_at.is_empty());
        assert_eq!(first_page.page, 1);
        assert_eq!(first_page.page_size, 1);
        assert_eq!(first_page.total_items, 2);
        assert_eq!(first_page.total_pages, 2);
        assert!(!first_page.has_previous_page);
        assert!(first_page.has_next_page);
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(first_page.items[0].release_run_id, running_release_run_id);
        assert_eq!(first_page.items[0].repository_name, "process-feed");
        assert_eq!(first_page.items[0].repository_engine_kind, "unity");
        assert_eq!(first_page.items[0].git_tag, "v1.1.0");
        assert_eq!(first_page.items[0].display_status, "running");
        assert_eq!(first_page.items[0].current_step_label, "Building Linux");
        assert_eq!(first_page.items[0].current_step_status, "running");
        assert_eq!(
            first_page.items[0].current_step_detail.as_deref(),
            Some("Packaging Linux player")
        );
        assert_eq!(first_page.items[0].running_build_runs, 1);
        assert_eq!(first_page.items[0].total_build_runs, 1);

        let second_page = list_process_feed_page(&layout, 2, 1)
            .expect("second process feed page should load");

        assert_eq!(second_page.page, 2);
        assert!(second_page.has_previous_page);
        assert!(!second_page.has_next_page);
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].release_run_id, completed_release_run_id);
        assert_eq!(second_page.items[0].display_status, "succeeded");
        assert_eq!(second_page.items[0].current_step_label, "Built Windows");
        assert_eq!(second_page.items[0].current_step_status, "succeeded");
        assert_eq!(second_page.items[0].succeeded_build_runs, 1);
        assert_eq!(second_page.items[0].total_publish_runs, 0);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_process_feed_page_filtered_supports_active_scope_and_archive_filters() {
        let root = test_root("list-process-feed-page-filtered");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "process-feed-filtered");

        let succeeded_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.0.0",
            ReleaseStatus::Succeeded.as_str(),
        );
        let succeeded_build_run_id = insert_build_run(
            &connection,
            succeeded_release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        update_build_run_plan(
            &connection,
            succeeded_build_run_id,
            "2022.3.18f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );

        let queued_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.1.0",
            ReleaseStatus::Queued.as_str(),
        );
        let queued_build_run_id = insert_build_run(
            &connection,
            queued_release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        update_build_run_plan(
            &connection,
            queued_build_run_id,
            "2022.3.18f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );

        let running_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.2.0",
            ReleaseStatus::Queued.as_str(),
        );
        let running_build_run_id = insert_build_run(
            &connection,
            running_release_run_id,
            fixture.secondary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        update_build_run_plan(
            &connection,
            running_build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );

        let failed_release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "blocked-v1.3.0",
            ReleaseStatus::Failed.as_str(),
        );
        let failed_build_run_id = insert_build_run(
            &connection,
            failed_release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Failed.as_str(),
        );
        update_build_run_plan(
            &connection,
            failed_build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        drop(connection);

        let active_page = list_process_feed_page_filtered(
            &layout,
            &ProcessFeedPageQuery {
                page: 1,
                page_size: 10,
                query: None,
                scope: ProcessFeedScope::Active,
                status: ProcessFeedStatusFilter::All,
            },
        )
        .expect("active process feed page should load");

        assert_eq!(active_page.total_items, 2);
        assert_eq!(active_page.items.len(), 2);
        assert_eq!(active_page.items[0].release_run_id, running_release_run_id);
        assert_eq!(active_page.items[0].display_status, "running");
        assert_eq!(active_page.items[1].release_run_id, queued_release_run_id);
        assert_eq!(active_page.items[1].display_status, "queued");

        let failed_archive_page = list_process_feed_page_filtered(
            &layout,
            &ProcessFeedPageQuery {
                page: 1,
                page_size: 10,
                query: Some(String::from("blocked")),
                scope: ProcessFeedScope::All,
                status: ProcessFeedStatusFilter::Failed,
            },
        )
        .expect("filtered archive process feed page should load");

        assert_eq!(failed_archive_page.total_items, 1);
        assert_eq!(failed_archive_page.items.len(), 1);
        assert_eq!(
            failed_archive_page.items[0].release_run_id,
            failed_release_run_id,
        );
        assert_eq!(failed_archive_page.items[0].display_status, "failed");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_artifact_inspection_records_returns_joined_artifact_activity() {
        let root = test_root("list-artifact-inspection-records");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "artifact-inspection");
        let release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v5.2.0",
            ReleaseStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    engine_version = ?
                WHERE id = ?
                ",
                params!["facefeed", "2022.3.20f1", release_run_id],
            )
            .expect("release metadata should update");
        connection
            .execute(
                "
                UPDATE build_targets
                SET contract_json = ?,
                    runner_type = ?
                WHERE id = ?
                ",
                params![
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "CI.Build.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                    DEFAULT_HOST_NATIVE_RUNNER_TYPE,
                    fixture.primary_build_target_id,
                ],
            )
            .expect("build target metadata should update");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        update_build_run_plan(
            &connection,
            build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET artifact_root_path = ?
                WHERE id = ?
                ",
                params!["C:/artifacts/artifact-inspection", build_run_id],
            )
            .expect("artifact root path should update");
        let artifact_id = insert_artifact(&connection, build_run_id, "artifact-inspection.zip");
        connection
            .execute(
                "
                UPDATE artifacts
                SET kind = ?,
                    path = ?,
                    active_location_ref = ?,
                    size_bytes = ?,
                    checksum_sha256 = ?
                WHERE id = ?
                ",
                params![
                    "archive",
                    "builds/windows/artifact-inspection.zip",
                    "builds/windows/artifact-inspection.zip",
                    4096_i64,
                    "abc123",
                    artifact_id,
                ],
            )
            .expect("artifact metadata should update");
        insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            fixture.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        connection
            .execute(
                "
                INSERT INTO publish_runs (
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status,
                    destination_ref
                )
                VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    fixture.publish_target_id,
                    artifact_id,
                    PublishStatus::Succeeded.as_str(),
                    "releases/windows/artifact-inspection.zip",
                ],
            )
            .expect("second publish run should insert");
        drop(connection);

        let records = list_artifact_inspection_records(&layout)
            .expect("artifact inspection should load joined artifact activity");

        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.artifact_id, artifact_id);
        assert_eq!(record.build_run_id, build_run_id);
        assert_eq!(record.release_run_id, release_run_id);
        assert_eq!(record.repository_id, fixture.repository_id);
        assert_eq!(record.repository_name, "artifact-inspection");
        assert_eq!(
            record.repository_url.as_deref(),
            Some("https://example.com/artifact-inspection.git")
        );
        assert_eq!(record.git_tag, "v5.2.0");
        assert_eq!(record.git_commit.as_deref(), Some("facefeed"));
        assert_eq!(record.build_target_id, fixture.primary_build_target_id);
        assert_eq!(record.build_target_name, "artifact-inspection-windows");
        assert_eq!(record.unity_target_platform, "StandaloneWindows64");
        assert_eq!(record.runner_type, DEFAULT_HOST_NATIVE_RUNNER_TYPE);
        assert_eq!(record.build_status, BuildStatus::Succeeded.as_str());
        assert_eq!(record.artifact_name, "artifact-inspection.zip");
        assert_eq!(record.artifact_kind, "archive");
        assert_eq!(
            record.artifact_path,
            "builds/windows/artifact-inspection.zip"
        );
        assert_eq!(
            record.artifact_root_path.as_deref(),
            Some("C:/artifacts/artifact-inspection")
        );
        assert_eq!(record.artifact_active_location_kind, "runtime_artifact");
        assert_eq!(
            record.artifact_active_location_ref,
            "builds/windows/artifact-inspection.zip"
        );
        assert_eq!(record.size_bytes, Some(4096));
        assert_eq!(record.checksum_sha256.as_deref(), Some("abc123"));
        assert_eq!(record.publish_run_count, 2);
        assert_eq!(record.queued_publish_runs, 1);
        assert_eq!(record.running_publish_runs, 0);
        assert_eq!(record.succeeded_publish_runs, 1);
        assert_eq!(record.failed_publish_runs, 0);
        assert_eq!(record.canceled_publish_runs, 0);
        assert_eq!(record.publish_runs.len(), 2);
        assert_eq!(record.publish_runs[0].status, PublishStatus::Queued.as_str());
        assert_eq!(record.publish_runs[0].publish_target_id, fixture.publish_target_id);
        assert_eq!(record.publish_runs[0].publish_target_kind, "filesystem");
        assert_eq!(
            record.publish_runs[0].publish_target_name,
            "artifact-inspection-publish"
        );
        assert_eq!(record.publish_runs[0].destination_ref, None);
        assert_eq!(record.publish_runs[1].status, PublishStatus::Succeeded.as_str());
        assert_eq!(
            record.publish_runs[1].destination_ref.as_deref(),
            Some("releases/windows/artifact-inspection.zip")
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_process_feed_page_marks_unstarted_builds_as_queued() {
        let root = test_root("list-process-feed-page-queued");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "process-feed-queued");
        let release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.2.0",
            ReleaseStatus::Queued.as_str(),
        );
        let queued_build_run_id = insert_build_run(
            &connection,
            release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        update_build_run_plan(
            &connection,
            queued_build_run_id,
            "2022.3.21f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        drop(connection);

        let page = list_process_feed_page(&layout, 1, 10)
            .expect("queued process feed page should load");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].release_run_id, release_run_id);
        assert_eq!(page.items[0].repository_engine_kind, "unity");
        assert_eq!(page.items[0].display_status, "queued");
        assert_eq!(page.items[0].current_step_label, "Queued build: Windows");
        assert_eq!(page.items[0].current_step_status, "queued");
        assert_eq!(page.items[0].queued_build_runs, 1);
        assert_eq!(page.items[0].running_build_runs, 0);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_process_feed_page_surfaces_publish_target_names() {
        let root = test_root("list-process-feed-page-publish-running");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "process-feed-publish");
        let release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.3.0",
            ReleaseStatus::Queued.as_str(),
        );
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        update_build_run_plan(
            &connection,
            build_run_id,
            "2022.3.22f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        let artifact_id = insert_artifact(&connection, build_run_id, "publishable.zip");
        let publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            fixture.publish_target_id,
            artifact_id,
            PublishStatus::Running.as_str(),
        );
        connection
            .execute(
                "
                UPDATE publish_runs
                SET started_at = ?
                WHERE id = ?
                ",
                params!["2026-01-12T09:00:00Z", publish_run_id],
            )
            .expect("running publish metadata should update");
        drop(connection);

        let page = list_process_feed_page(&layout, 1, 10)
            .expect("publish process feed page should load");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].release_run_id, release_run_id);
        assert_eq!(page.items[0].display_status, "running");
        assert_eq!(
            page.items[0].current_step_label,
            "Publishing process-feed-publish-publish"
        );
        assert_eq!(page.items[0].current_step_status, "running");
        assert_eq!(page.items[0].running_publish_runs, 1);
        assert_eq!(page.items[0].total_publish_runs, 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_credential_records_returns_stored_credentials_in_name_order() {
        let root = test_root("list-credential-records");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "zeta-token",
                    "git-http-bearer",
                    r#"{"token":"solidarity"}"#,
                ],
            )
            .expect("first credentials row should insert");
        let second_id = connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "alpha-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("second credentials row should insert");
        let _ = second_id;
        drop(connection);

        let credentials = list_credential_records(&layout)
            .expect("credential settings should load");

        assert_eq!(credentials.len(), 2);
        assert_eq!(
            credentials[0],
            CredentialRecord {
                id: 2,
                name: String::from("alpha-basic"),
                kind: String::from("git-http-basic"),
                config_json: String::from(
                    r#"{"username":"worker","password":"solidarity"}"#,
                ),
                created_at: credentials[0].created_at.clone(),
                updated_at: credentials[0].updated_at.clone(),
            }
        );
        assert_eq!(credentials[1].name, "zeta-token");
        assert_eq!(credentials[1].kind, "git-http-bearer");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_publish_target_runtime_settings_returns_bound_credentials() {
        let root = test_root("list-publish-target-runtime-settings");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "publish-settings");
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    "publish-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        connection
            .execute(
                "
                UPDATE publish_targets
                SET credentials_id = ?, enabled = 0
                WHERE id = ?
                ",
                params![credentials_id, fixture.publish_target_id],
            )
            .expect("publish target credentials should update");
        drop(connection);

        let targets = list_publish_target_runtime_settings(&layout)
            .expect("publish target settings should load");

        assert_eq!(targets.len(), 1);
        assert_eq!(
            targets[0],
            PublishTargetRuntimeSettingsRecord {
                id: fixture.publish_target_id,
                repository_id: fixture.repository_id,
                repository_name: String::from("publish-settings"),
                name: String::from("publish-settings-publish"),
                kind: String::from("filesystem"),
                config_json: String::from("{}"),
                credentials_id: Some(credentials_id),
                enabled: false,
            }
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_publish_target_binding_runtime_settings_reports_consumption_behavior() {
        let root = test_root("publish-binding-settings");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);

        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "publish-binding-settings");
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
                    fixture.primary_build_target_id,
                    fixture.publish_target_id,
                    1_i64,
                    r#"{"operation":"move","directory_path":"D:/published"}"#,
                ],
            )
            .expect("consuming publish binding should insert");
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
                    fixture.secondary_build_target_id,
                    fixture.publish_target_id,
                    0_i64,
                    "{}",
                ],
            )
            .expect("non-consuming publish binding should insert");
        drop(connection);

        let bindings = list_publish_target_binding_runtime_settings(&layout)
            .expect("publish binding settings should load");

        assert_eq!(bindings.len(), 2);

        let non_consuming = bindings
            .iter()
            .find(|binding| binding.build_target_id == fixture.secondary_build_target_id)
            .expect("secondary build target binding should exist");
        assert_eq!(non_consuming.publish_target_id, fixture.publish_target_id);
        assert_eq!(non_consuming.repository_id, fixture.repository_id);
        assert_eq!(
            non_consuming.build_target_name,
            "publish-binding-settings-linux"
        );
        assert!(!non_consuming.enabled);
        assert_eq!(non_consuming.options_json, "{}");
        assert_eq!(non_consuming.consumption_behavior, "non_consuming");

        let consuming = bindings
            .iter()
            .find(|binding| binding.build_target_id == fixture.primary_build_target_id)
            .expect("primary build target binding should exist");
        assert_eq!(consuming.publish_target_id, fixture.publish_target_id);
        assert_eq!(consuming.repository_id, fixture.repository_id);
        assert_eq!(consuming.build_target_name, "publish-binding-settings-windows");
        assert!(consuming.enabled);
        assert_eq!(
            consuming.options_json,
            r#"{"operation":"move","directory_path":"D:/published"}"#
        );
        assert_eq!(consuming.consumption_behavior, "consuming");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn initialize_database_is_idempotent() {
        let root = test_root("idempotent");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);

        initialize_database(&layout).expect("first bootstrap should succeed");
        let second = initialize_database(&layout).expect("second bootstrap should succeed");

        assert!(second.applied_migrations.is_empty());

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let migration_count: i64 = connection
            .query_row("SELECT COUNT(1) FROM schema_migrations", [], |row| row.get(0))
            .expect("migration count should be readable");

        assert_eq!(migration_count, MIGRATIONS.len() as i64);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn initialize_database_upgrades_existing_repositories_to_source_configuration() {
        let root = test_root("upgrade-repository-source-configuration");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);

        std::fs::create_dir_all(&directories.state_dir)
            .expect("state directory should create for upgrade fixture");
        let mut connection = Connection::open(&layout.database_path)
            .expect("upgrade fixture database should open");
        apply_pragmas(&connection).expect("pragmas should apply to upgrade fixture");
        ensure_migration_ledger(&connection).expect("migration ledger should create");

        for migration in MIGRATIONS.iter().take(5) {
            let transaction = connection
                .transaction()
                .expect("upgrade fixture transaction should start");
            transaction
                .execute_batch(migration.sql)
                .expect("upgrade fixture SQL should apply");
            transaction
                .execute(
                    "
                    INSERT INTO schema_migrations (name, applied_at)
                    VALUES (?, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                    ",
                    params![migration.name],
                )
                .expect("upgrade fixture ledger row should insert");
            transaction
                .commit()
                .expect("upgrade fixture transaction should commit");
        }

        connection
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    repo_url,
                    credentials_id,
                    default_branch,
                    polling_interval_seconds,
                    last_seen_tag,
                    enabled
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    "existing-managed-repository",
                    "https://example.com/existing-managed-repository.git",
                    Option::<i64>::None,
                    "main",
                    900,
                    "v1.2.3",
                    1,
                ],
            )
            .expect("existing repository row should insert");
        drop(connection);

        let report = initialize_database(&layout).expect("upgrade bootstrap should succeed");

        assert_eq!(
            report.applied_migrations,
            vec![
                "0006_repository_source_configuration.sql",
                "0007_repository_path_model_cleanup.sql",
                "0008_build_run_stage_tracking.sql",
                "0009_build_target_runner_model_cleanup.sql",
                "0009_execution_retention.sql",
                "0010_engine_contract_model.sql",
                "0011_runtime_engine_version.sql",
                "0012_repository_auth_state.sql",
                "0013_publish_destination_execution_contract.sql",
                "0014_release_source_identity.sql",
                "0015_local_workspace_polling_invariant.sql"
            ]
        );

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert!(table_exists(&connection, "app_settings"));
        assert!(index_exists(
            &connection,
            "idx_repositories_repo_url_unique"
        ));
        assert!(index_exists(
            &connection,
            "idx_repositories_local_path_unique"
        ));
        assert!(index_exists(&connection, "idx_repositories_source_mode"));

        let repository_row: (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
            Option<String>,
            i64,
        ) = connection
            .query_row(
                "
                SELECT name,
                       source_mode,
                       workspace_strategy,
                       repo_url,
                       local_path,
                       default_branch,
                       polling_interval_seconds,
                       last_seen_tag,
                       enabled
                FROM repositories
                WHERE name = ?
                ",
                ["existing-managed-repository"],
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
                        row.get(8)?,
                    ))
                },
            )
            .expect("upgraded repository row should load");

        assert_eq!(repository_row.0, "existing-managed-repository");
        assert_eq!(repository_row.1, "managed_repository");
        assert_eq!(repository_row.2, "managed_checkout");
        assert_eq!(
            repository_row.3.as_deref(),
            Some("https://example.com/existing-managed-repository.git")
        );
        assert!(repository_row.4.is_none());
        assert_eq!(repository_row.5.as_deref(), Some("main"));
        assert_eq!(repository_row.6, 900);
        assert_eq!(repository_row.7.as_deref(), Some("v1.2.3"));
        assert_eq!(repository_row.8, 1);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn automation_snapshot_reports_queue_repository_and_lease_state() {
        let root = test_root("automation-snapshot");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repository = seed_repository_fixture(&connection, "automation");
        let release_run_id = insert_release_run(
            &connection,
            repository.repository_id,
            "v1.2.3",
            ReleaseStatus::Queued.as_str(),
        );
        update_release_run_engine_version(&connection, release_run_id, "2022.3.20f1");

        let queued_build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repository.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        let running_build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repository.secondary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        update_build_run_plan(
            &connection,
            queued_build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        update_build_run_plan(
            &connection,
            running_build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );

        let artifact_id = insert_artifact(&connection, queued_build_run_id, "game.zip");
        insert_publish_run(
            &connection,
            release_run_id,
            queued_build_run_id,
            repository.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        coordinator
            .enqueue(RELEASE_RUN_QUEUE_NAME, br#"{"release_run_id":1}"#)
            .expect("release queue message should enqueue");
        coordinator
            .enqueue(BUILD_RUN_QUEUE_NAME, br#"{"build_run_id":1}"#)
            .expect("build queue message should enqueue");
        coordinator
            .enqueue(PUBLISH_RUN_QUEUE_NAME, br#"{"publish_run_id":1}"#)
            .expect("publish queue message should enqueue");
        let claimed_publish_message = coordinator
            .claim_next(
                PUBLISH_RUN_QUEUE_NAME,
                "publish-worker",
                Duration::ZERO,
                Duration::from_secs(30),
            )
            .expect("publish queue claim should succeed")
            .expect("publish queue message should be available");
        let lease = coordinator
            .acquire_lock("release-plan:automation", Duration::from_secs(30))
            .expect("coordination lease should succeed")
            .expect("coordination lease should be created");

        let snapshot = coordinator
            .automation_snapshot()
            .expect("automation snapshot should succeed");

        assert!(!snapshot.generated_at.is_empty());
        assert_eq!(snapshot.repositories.len(), 1);

        let repository = &snapshot.repositories[0];
        assert_eq!(repository.repository_name, "automation");
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
            .find(|queue| queue.queue_name == BUILD_RUN_QUEUE_NAME)
            .expect("build queue snapshot should exist");
        assert_eq!(build_queue.ready_count, 1);
        assert_eq!(build_queue.leased_count, 0);

        let publish_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == PUBLISH_RUN_QUEUE_NAME)
            .expect("publish queue snapshot should exist");
        assert_eq!(publish_queue.ready_count, 0);
        assert_eq!(publish_queue.leased_count, 1);

        let release_queue = snapshot
            .queue_messages
            .iter()
            .find(|queue| queue.queue_name == RELEASE_RUN_QUEUE_NAME)
            .expect("release queue snapshot should exist");
        assert_eq!(release_queue.ready_count, 1);
        assert_eq!(release_queue.leased_count, 0);

        assert_eq!(snapshot.coordination_leases.len(), 1);
        assert_eq!(snapshot.coordination_leases[0].name, lease.name);
        assert!(
            snapshot.coordination_leases[0].lease_expires_at_unix_millis
                >= lease.lease_expires_at_unix_millis
        );

        assert_eq!(claimed_publish_message.queue_name, PUBLISH_RUN_QUEUE_NAME);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn local_queue_claims_release_and_acknowledge_messages() {
        let root = test_root("queue");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .enqueue("builds", br#"{"job":"build"}"#)
            .expect("enqueue should succeed");

        let first = coordinator
            .claim_next(
                "builds",
                "worker-a",
                Duration::ZERO,
                Duration::from_millis(400),
            )
            .expect("claim should succeed")
            .expect("message should be available");
        assert_eq!(first.payload, br#"{"job":"build"}"#);
        assert_eq!(first.dequeue_count, 1);

        let blocked = coordinator
            .claim_next(
                "builds",
                "worker-b",
                Duration::ZERO,
                Duration::from_millis(400),
            )
            .expect("second claim should succeed");
        assert!(blocked.is_none());

        assert!(coordinator
            .renew_message_lease(
                first.id,
                &first.lease_token,
                Duration::from_millis(80),
            )
            .expect("renew should succeed"));
        assert!(coordinator
            .release_message(first.id, &first.lease_token)
            .expect("release should succeed"));

        let retried = coordinator
            .claim_next(
                "builds",
                "worker-c",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("retry claim should succeed")
            .expect("released message should be available again");
        assert_eq!(retried.dequeue_count, 2);
        assert!(coordinator
            .acknowledge_message(retried.id, &retried.lease_token)
            .expect("acknowledge should succeed"));

        let empty = coordinator
            .claim_next(
                "builds",
                "worker-d",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("empty claim should succeed");
        assert!(empty.is_none());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn local_queue_reclaims_expired_leases() {
        let root = test_root("queue-expiry");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .enqueue("publishes", br#"{"job":"publish"}"#)
            .expect("enqueue should succeed");

        let claimed = coordinator
            .claim_next(
                "publishes",
                "worker-a",
                Duration::ZERO,
                Duration::from_millis(20),
            )
            .expect("claim should succeed")
            .expect("message should be available");

        std::thread::sleep(Duration::from_millis(35));

        let retried = coordinator
            .claim_next(
                "publishes",
                "worker-b",
                Duration::ZERO,
                Duration::from_millis(20),
            )
            .expect("second claim should succeed")
            .expect("expired lease should make message claimable");
        assert_eq!(retried.id, claimed.id);
        assert_eq!(retried.dequeue_count, 2);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn build_job_claim_respects_host_capacity() {
        let root = test_root("build-claim-capacity");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let running_fixture = seed_build_claim_fixture(
            &connection,
            "running-repo",
            "v1.0.0",
            BuildStatus::Running.as_str(),
        );
        let queued_fixture = seed_build_claim_fixture(
            &connection,
            "queued-repo",
            "v2.0.0",
            BuildStatus::Queued.as_str(),
        );
        enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, queued_fixture.build_run_id).as_bytes(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let claimed = coordinator
            .claim_next_build_job(
                "worker-a",
                Duration::ZERO,
                Duration::from_millis(40),
                &RuntimeConcurrencySettings::development(),
            )
            .expect("build claim should succeed");
        assert!(claimed.is_none());
        assert!(running_fixture.build_run_id > 0);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn build_run_dispatch_enqueues_compatible_payload_and_is_idempotent() {
        let root = test_root("build-dispatch");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "dispatch-build-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v3.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        update_build_run_plan(
            &connection,
            build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        assert_eq!(
            coordinator
                .dispatch_build_run(build_run_id)
                .expect("first build dispatch should succeed"),
            QueueDispatchOutcome::Enqueued,
        );
        assert_eq!(
            coordinator
                .dispatch_build_run(build_run_id)
                .expect("duplicate build dispatch should succeed"),
            QueueDispatchOutcome::AlreadyClaimed,
        );

        let claimed = coordinator
            .claim_next_build_job(
                "build-worker-a",
                Duration::ZERO,
                Duration::from_millis(40),
                &RuntimeConcurrencySettings::development(),
            )
            .expect("build claim should succeed")
            .expect("queued build job should be claimable");
        let decoded: Value =
            serde_json::from_slice(&claimed.payload).expect("payload should decode as JSON");
        assert_eq!(decoded["build_run_id"], build_run_id);
        assert_eq!(decoded["release_run_id"], release_run_id);
        assert_eq!(decoded["build_target_id"], repo.primary_build_target_id);
        assert_eq!(decoded["engine_version"], "2022.3.20f1");
        assert_eq!(decoded["image_ref"], DEFAULT_HOST_NATIVE_RUNNER_TYPE);

        let job: BuildDispatchJob =
            serde_json::from_slice(&claimed.payload).expect("payload should match build job contract");
        assert_eq!(job.build_run_id, build_run_id);
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn dispatch_manual_release_enqueues_release_job_and_marks_queued() {
        let root = test_root("release-dispatch");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "manual-release-repo");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let release = coordinator
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id: repo.repository_id,
                git_tag: "v6.0.0".to_owned(),
                git_commit: "cafebabe".to_owned(),
                requested_via: "cli".to_owned(),
            })
            .expect("manual release dispatch should succeed");
        assert_eq!(release.status, ReleaseStatus::Queued.as_str());
        assert_eq!(release.trigger_source, TRIGGER_SOURCE_MANUAL);
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 1);

        let claimed = coordinator
            .claim_next(
                RELEASE_RUN_QUEUE_NAME,
                "release-planner-a",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("release claim should succeed")
            .expect("release queue message should be available");
        let job: ReleaseDispatchJob = serde_json::from_slice(&claimed.payload)
            .expect("release queue payload should decode");
        assert_eq!(job.release_run_id, release.id);
        assert_eq!(job.repository_id, repo.repository_id);
        assert_eq!(job.git_tag, "v6.0.0");
        assert_eq!(job.git_commit.as_deref(), Some("cafebabe"));
        assert_eq!(job.trigger_source, TRIGGER_SOURCE_MANUAL);

        let metadata: Value = serde_json::from_str(&release.source_metadata_json)
            .expect("manual release metadata should decode");
        assert_eq!(metadata["requested_via"], "cli");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn dispatch_manual_release_rejects_duplicate_repository_tag() {
        let root = test_root("release-dispatch-conflict");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "manual-release-conflict");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id: repo.repository_id,
                git_tag: "v6.1.0".to_owned(),
                git_commit: String::new(),
                requested_via: "hub".to_owned(),
            })
            .expect("first manual release dispatch should succeed");

        let error = coordinator
            .dispatch_manual_release(ManualReleaseDispatchInput {
                repository_id: repo.repository_id,
                git_tag: "v6.1.0".to_owned(),
                git_commit: String::new(),
                requested_via: "hub".to_owned(),
            })
            .expect_err("duplicate manual release dispatch should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn dispatch_manual_release_rebuild_reuses_release_and_clears_derived_state() {
        let root = test_root("release-dispatch-rebuild");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "manual-release-rebuild");
        let release_run_id = seed_manual_release_for_rebuild(&connection, repo.repository_id, "v7.0.0");
        let workspace_path = root.join("runs").join("build-run-41");
        let artifact_root_path = root.join("artifacts").join("release-v7.0.0");
        std::fs::create_dir_all(workspace_path.join("source"))
            .expect("workspace source directory should create");
        std::fs::create_dir_all(&artifact_root_path)
            .expect("artifact root directory should create");
        std::fs::write(workspace_path.join("source").join("build.txt"), "workspace")
            .expect("workspace marker should write");
        std::fs::write(artifact_root_path.join("rebuilt.zip"), "artifact")
            .expect("artifact marker should write");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET workspace_path = ?,
                    artifact_root_path = ?
                WHERE id = ?
                ",
                params![
                    workspace_path.display().to_string(),
                    artifact_root_path.display().to_string(),
                    build_run_id,
                ],
            )
            .expect("build run paths should update");
        let artifact_id = insert_artifact(&connection, build_run_id, "rebuilt.zip");
        insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Succeeded.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        assert!(coordinator
            .claim_idempotency(
                &release_dispatch_idempotency_key(release_run_id),
                Duration::from_millis(100),
            )
            .expect("seed idempotency claim should succeed"));

        let rebuilt = coordinator
            .dispatch_manual_release_rebuild(ManualReleaseDispatchInput {
                repository_id: repo.repository_id,
                git_tag: "v7.0.0".to_owned(),
                git_commit: "feedface".to_owned(),
                requested_via: "hub".to_owned(),
            })
            .expect("manual release rebuild should succeed");
        assert_eq!(rebuilt.id, release_run_id);
        assert_eq!(rebuilt.status, ReleaseStatus::Queued.as_str());
        assert_eq!(rebuilt.git_commit.as_deref(), Some("feedface"));
        assert_eq!(rebuilt.engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 1);
        assert!(!coordinator
            .claim_idempotency(
                &release_dispatch_idempotency_key(release_run_id),
                Duration::from_millis(100),
            )
            .expect("rebuilt dispatch should reclaim idempotency"));

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert_eq!(build_run_count_for_release(&connection, release_run_id), 0);
        assert_eq!(publish_run_count_for_release(&connection, release_run_id), 0);
        let persisted = load_release_record(&connection, release_run_id);
        assert_eq!(persisted.id, release_run_id);
        assert_eq!(persisted.status, ReleaseStatus::Queued.as_str());
        assert_eq!(persisted.git_commit.as_deref(), Some("feedface"));
        assert_eq!(persisted.engine_version.as_deref(), Some("2022.3.20f1"));
        let metadata: Value = serde_json::from_str(&persisted.source_metadata_json)
            .expect("rebuild metadata should decode");
        assert_eq!(metadata["requested_via"], "hub");
        drop(connection);

        assert!(!workspace_path.exists());
        assert!(!artifact_root_path.exists());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn process_next_release_job_plans_build_runs_and_acknowledges_message() {
        let root = test_root("release-queue-consumer");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "release-queue-consumer");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v7.1.0",
            ReleaseStatus::Detected.as_str(),
        );
        update_release_run_engine_version(&connection, release_run_id, "2022.3.20f1");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .queue_release_run(release_run_id)
            .expect("release queue handoff should succeed");

        assert!(coordinator
            .process_next_release_job(
                "release-planner-a",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("release queue consumer should succeed"));
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 0);
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 1);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert_eq!(build_run_count_for_release(&connection, release_run_id), 2);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn process_next_release_job_acknowledges_release_without_enabled_targets() {
        let root = test_root("release-queue-no-enabled-targets");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "release-queue-no-enabled-targets");
        connection
            .execute(
                "UPDATE build_targets SET enabled = 0 WHERE repository_id = ?",
                [repo.repository_id],
            )
            .expect("build targets should disable");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v7.1.1",
            ReleaseStatus::Detected.as_str(),
        );
        update_release_run_engine_version(&connection, release_run_id, "2022.3.20f1");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .queue_release_run(release_run_id)
            .expect("release queue handoff should succeed");

        assert!(coordinator
            .process_next_release_job(
                "release-planner-no-targets",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("release queue consumer should treat missing enabled targets as a no-op"));
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 0);
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 0);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert_eq!(build_run_count_for_release(&connection, release_run_id), 0);
        let release = load_release_record(&connection, release_run_id);
        assert_eq!(release.status, ReleaseStatus::Queued.as_str());
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn process_next_release_job_releases_message_when_repository_lane_is_busy() {
        let root = test_root("release-queue-lane-busy");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "release-queue-lane-busy");
        let first_release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v8.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        update_release_run_engine_version(&connection, first_release_run_id, "2022.3.20f1");
        let second_release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v8.1.0",
            ReleaseStatus::Detected.as_str(),
        );
        update_release_run_engine_version(&connection, second_release_run_id, "2022.3.20f1");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .plan_release_builds(first_release_run_id)
            .expect("first release should plan build runs");
        coordinator
            .queue_release_run(second_release_run_id)
            .expect("second release should queue successfully");

        assert!(coordinator
            .process_next_release_job(
                "release-planner-b",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("release queue consumer should handle busy repository"));
        assert_eq!(queue_message_count(&layout.database_path, RELEASE_RUN_QUEUE_NAME), 1);

        let claimed = coordinator
            .claim_next(
                RELEASE_RUN_QUEUE_NAME,
                "release-planner-c",
                Duration::ZERO,
                Duration::from_millis(40),
            )
            .expect("released release message should be claimable again")
            .expect("released release message should exist");
        let job = decode_release_dispatch_job(&claimed.payload)
            .expect("released queue payload should decode");
        assert_eq!(job.release_run_id, second_release_run_id);
        drop(claimed);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert_eq!(build_run_count_for_release(&connection, second_release_run_id), 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_materializes_runs_and_dispatches_once() {
        let root = test_root("plan-release-builds");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "planning-repo");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v5.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        update_release_run_engine_version(&connection, release_run_id, "2022.3.20f1");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let first_plan = coordinator
            .plan_release_builds(release_run_id)
            .expect("release planning should succeed");
        assert_planned_build_runs(
            &first_plan,
            release_run_id,
            repo.primary_build_target_id,
            repo.secondary_build_target_id,
        );
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 1);

        let limits = RuntimeConcurrencySettings {
            max_concurrent_build_runs: 2,
            max_concurrent_publish_runs: 1,
            max_active_releases_per_repository: 1,
        };
        let first_claim = coordinator
            .claim_next_build_job(
                "planner-worker-a",
                Duration::ZERO,
                Duration::from_millis(400),
                &limits,
            )
            .expect("first queued build should claim")
            .expect("first queued build should exist");
        let second_claim = coordinator
            .claim_next_build_job(
                "planner-worker-b",
                Duration::ZERO,
                Duration::from_millis(400),
                &limits,
            )
            .expect("second queued build lookup should succeed");
        let first_job: BuildDispatchJob = serde_json::from_slice(&first_claim.payload)
            .expect("first queue payload should decode as build job");
        assert_eq!(first_job.build_run_id, first_plan[0].id);
        assert!(second_claim.is_none());

        coordinator
            .start_build_run(
                first_job.build_run_id,
                StartBuildRunInput {
                    workspace_path: String::from("/tmp/runs/release-run-1"),
                    log_path: String::from("/tmp/logs/build-run-1.log"),
                    artifact_root_path: String::from("/tmp/runs/release-run-1/outputs"),
                },
            )
            .expect("first planned build should start");
        coordinator
            .complete_build_run(
                first_job.build_run_id,
                CompleteBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                },
            )
            .expect("first planned build should complete");

        let second_claim = coordinator
            .claim_next_build_job(
                "planner-worker-b",
                Duration::ZERO,
                Duration::from_millis(400),
                &limits,
            )
            .expect("second queued build should claim after the first terminal build")
            .expect("second queued build should exist after the first terminal build");
        let second_job: BuildDispatchJob = serde_json::from_slice(&second_claim.payload)
            .expect("second queue payload should decode as build job");
        assert_eq!(second_job.build_run_id, first_plan[1].id);

        let second_plan = coordinator
            .plan_release_builds(release_run_id)
            .expect("repeated release planning should stay idempotent");
        assert_eq!(
            second_plan.iter().map(|run| run.id).collect::<Vec<_>>(),
            first_plan.iter().map(|run| run.id).collect::<Vec<_>>(),
        );
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 2);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_detects_and_persists_unity_version_from_git_repository() {
        let root = test_root("plan-release-builds-unity");
        let _env_lock = test_environment_lock()
            .lock()
            .expect("test environment lock should acquire");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let repository_url = create_tagged_unity_repository(
            &root.join("planning-repo-unity-source"),
            "v5.1.0",
            "2021.3.33f1",
        );

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture_with_url(
            &connection,
            "planning-repo-unity",
            &repository_url,
        );
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v5.1.0",
            ReleaseStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_release_builds(release_run_id)
            .expect("planning should detect unity version from the repository tag");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].engine_version.as_deref(), Some("2021.3.33f1"));
        assert_eq!(runs[1].engine_version.as_deref(), Some("2021.3.33f1"));
        assert_eq!(
            runs[0].image_ref.as_deref(),
            Some(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
        );
        assert_eq!(
            runs[1].image_ref.as_deref(),
            Some(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
        );

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let release = load_release_record(&connection, release_run_id);
        assert_eq!(release.engine_version.as_deref(), Some("2021.3.33f1"));
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_uses_repository_credentials_for_unity_version_detection() {
        let root = test_root("plan-release-builds-authenticated-unity");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture_with_url(
            &connection,
            "planning-private-repo-unity",
            "https://example.com/private-repo.git",
        );
        let credentials_id = insert_git_bearer_credentials(
            &connection,
            "planning-private-repo-pat",
            "planning-secret-token",
        );
        connection
            .execute(
                "UPDATE repositories SET credentials_id = ? WHERE id = ?",
                params![credentials_id, repo.repository_id],
            )
            .expect("repository credentials should update");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v6.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        drop(connection);

        let fake_git = FakeGitFixture::install(
            &root,
            "planning-secret-token",
            "2022.3.20f1",
        );
        let coordinator = LocalCoordinator::new(&layout);

        let runs = coordinator
            .plan_release_builds(release_run_id)
            .expect("planning should use repository credentials for unity detection");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(runs[1].engine_version.as_deref(), Some("2022.3.20f1"));

        let git_log = std::fs::read_to_string(fake_git.log_path())
            .expect("fake git log should be readable");
        assert!(git_log.contains(
            "http.extraHeader=Authorization: Bearer planning-secret-token"
        ));
        assert!(
            git_log.matches("planning-secret-token").count() >= 2,
            "git auth should be applied to clone and checkout commands"
        );

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let release = load_release_record(&connection, release_run_id);
        assert_eq!(release.engine_version.as_deref(), Some("2022.3.20f1"));
        drop(connection);

        drop(fake_git);
        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_accepts_host_native_runner_targets() {
        let root = test_root("plan-release-builds-host-native");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "planning-repo-host-native");
        connection
            .execute(
                "
                UPDATE build_targets
                SET runner_type = ?
                WHERE repository_id = ?
                ",
                params![crate::DEFAULT_HOST_NATIVE_RUNNER_TYPE, repo.repository_id],
            )
            .expect("host-native runner type should update");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v5.2.0",
            ReleaseStatus::Queued.as_str(),
        );
        connection
            .execute(
                "UPDATE release_runs SET engine_version = ? WHERE id = ?",
                params!["2022.3.20f1", release_run_id],
            )
            .expect("release unity version should update");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_release_builds(release_run_id)
            .expect("planning should support host-native runner targets");

        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].image_ref.as_deref(),
            Some(crate::DEFAULT_HOST_NATIVE_RUNNER_TYPE)
        );
        assert_eq!(
            runs[1].image_ref.as_deref(),
            Some(crate::DEFAULT_HOST_NATIVE_RUNNER_TYPE)
        );
        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_prefers_contract_editor_version_over_release_engine_version() {
        let root = test_root("plan-release-builds-contract-editor-version");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "planning-repo-contract-editor-version");
        connection
            .execute(
                "
                UPDATE build_targets
                SET build_kind = ?,
                    contract_json = ?
                WHERE id = ?
                ",
                params![
                    "player",
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.Perform",
                            "editorVersion": "6000.1.5f1"
                        }
                    })
                    .to_string(),
                    repo.primary_build_target_id,
                ],
            )
            .expect("primary build target contract should update");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v5.3.0",
            ReleaseStatus::Queued.as_str(),
        );
        connection
            .execute(
                "UPDATE release_runs SET engine_version = ? WHERE id = ?",
                params!["2022.3.20f1", release_run_id],
            )
            .expect("release unity version should update");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_release_builds(release_run_id)
            .expect("planning should prefer contract editorVersion when present");

        let primary = runs
            .iter()
            .find(|run| run.build_target_id == repo.primary_build_target_id)
            .expect("primary build run should exist");
        let secondary = runs
            .iter()
            .find(|run| run.build_target_id == repo.secondary_build_target_id)
            .expect("secondary build run should exist");

        assert_eq!(primary.engine_version.as_deref(), Some("6000.1.5f1"));
        assert_eq!(secondary.engine_version.as_deref(), Some("2022.3.20f1"));

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_release_builds_rejects_missing_unity_contract_payload() {
        let root = test_root("plan-release-builds-missing-unity-contract");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "planning-repo-missing-unity-contract");
        connection
            .execute(
                "
                UPDATE build_targets
                SET build_kind = ?,
                    contract_json = ?
                WHERE id = ?
                ",
                params!["player", "{}", repo.primary_build_target_id],
            )
            .expect("primary build target contract should update");
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v5.4.0",
            ReleaseStatus::Queued.as_str(),
        );
        connection
            .execute(
                "UPDATE release_runs SET engine_version = ? WHERE id = ?",
                params!["2022.3.20f1", release_run_id],
            )
            .expect("release unity version should update");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let error = coordinator
            .plan_release_builds(release_run_id)
            .expect_err("planning should reject build targets without a Unity contract payload");

        assert!(
            error
                .to_string()
                .contains("missing contract_json.unity for Unity planning")
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }


    #[test]
    fn get_build_execution_plan_loads_joined_metadata() {
        let root = test_root("build-execution-plan");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "build-execution-plan");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let plan = coordinator
            .get_build_execution_plan(fixture.build_run_id)
            .expect("build execution plan should load");

        assert_eq!(
            plan,
            BuildExecutionPlan {
                build_run_id: fixture.build_run_id,
                release_run_id: fixture.release_run_id,
                repository_id: fixture.repository_id,
                engine_kind: EngineKind::Unity,
                repository_name: String::from("build-execution-plan"),
                repository_credentials_id: None,
                workspace_root_override: None,
                artifacts_root_override: None,
                build_target_id: fixture.build_target_id,
                repository_source_mode: String::from("managed_repository"),
                repository_url: String::from("https://example.com/build-execution-plan.git"),
                repository_local_path: None,
                git_tag: String::from("v10.0.0"),
                git_commit: Some(String::from("deadbeef")),
                source_metadata_json: String::from("{}"),
                target_name: String::from("build-execution-plan-windows"),
                build_kind: BuildKind::Player,
                contract_json: serde_json::json!({
                    "unity": {
                        "targetPlatform": "StandaloneWindows64",
                        "buildMethod": "CI.Build.Perform",
                        "editorVersion": ""
                    }
                })
                .to_string(),
                runner_type: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                output_kind: Some(String::from("archive")),
                output_path_template: Some(String::from("players/game.zip")),
                config_json: String::from(r#"{"optimize":true}"#),
                engine_version: String::from("2022.3.20f1"),
                image_ref: String::from(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
                timeout_seconds: 900,
                status: String::from("queued"),
            }
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn get_credential_record_loads_stored_git_auth_configuration() {
        let root = test_root("credential-record");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        connection
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                VALUES (?, ?, ?)
                ",
                params![
                    "git-basic",
                    "git-http-basic",
                    r#"{"username":"worker","password":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let record = coordinator
            .get_credential_record(credentials_id)
            .expect("credential record should load");

        assert_eq!(record.id, credentials_id);
        assert_eq!(record.name, "git-basic");
        assert_eq!(record.kind, "git-http-basic");
        assert_eq!(
            record.config_json,
            r#"{"username":"worker","password":"solidarity"}"#,
        );
        assert!(!record.created_at.is_empty());
        assert!(!record.updated_at.is_empty());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn upsert_credential_record_creates_and_updates_credentials() {
        let root = test_root("upsert-credential-record");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        let created = coordinator
            .upsert_credential_record(UpsertCredentialRecordInput {
                credential_id: None,
                name: String::from("origin-basic"),
                kind: String::from("git-http-basic"),
                config_json: String::from(
                    r#"{"username":"worker","password":"solidarity"}"#,
                ),
            })
            .expect("credential should create");

        assert!(created.id > 0);
        assert_eq!(created.name, "origin-basic");
        assert_eq!(created.kind, "git-http-basic");

        let updated = coordinator
            .upsert_credential_record(UpsertCredentialRecordInput {
                credential_id: Some(created.id),
                name: String::from("origin-rotated"),
                kind: String::from("git-http-basic"),
                config_json: String::from(
                    r#"{"username":"worker","password":"new-secret"}"#,
                ),
            })
            .expect("credential should update");

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "origin-rotated");
        assert_eq!(
            updated.config_json,
            r#"{"username":"worker","password":"new-secret"}"#,
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn update_credentials_bindings_retargets_repository_and_publish_target() {
        let root = test_root("update-credentials-bindings");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "credential-bindings");
        connection
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                VALUES (?, ?, ?)
                ",
                params![
                    "publish-bearer",
                    "git-http-bearer",
                    r#"{"token":"solidarity"}"#,
                ],
            )
            .expect("credentials row should insert");
        let credentials_id = connection.last_insert_rowid();
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .update_repository_credentials_binding(
                fixture.repository_id,
                Some(credentials_id),
            )
            .expect("repository binding should update");
        coordinator
            .update_publish_target_credentials_binding(
                fixture.publish_target_id,
                Some(credentials_id),
            )
            .expect("publish target binding should update");

        let repositories = coordinator
            .list_polling_repositories()
            .expect("repositories should load");
        let publish_targets = list_publish_target_runtime_settings(&layout)
            .expect("publish targets should load");

        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].credentials_id, Some(credentials_id));
        assert_eq!(publish_targets.len(), 1);
        assert_eq!(publish_targets[0].credentials_id, Some(credentials_id));

        coordinator
            .update_repository_credentials_binding(fixture.repository_id, None)
            .expect("repository binding should clear");
        coordinator
            .update_publish_target_credentials_binding(fixture.publish_target_id, None)
            .expect("publish target binding should clear");

        let repositories = coordinator
            .list_polling_repositories()
            .expect("repositories should reload");
        let publish_targets = list_publish_target_runtime_settings(&layout)
            .expect("publish targets should reload");

        assert_eq!(repositories[0].credentials_id, None);
        assert_eq!(publish_targets[0].credentials_id, None);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn start_and_complete_build_run_persist_execution_paths() {
        let root = test_root("build-run-complete");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "build-run-complete");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let running = coordinator
            .start_build_run(
                fixture.build_run_id,
                StartBuildRunInput {
                    workspace_path: String::from("/tmp/runs/build-run-complete"),
                    log_path: String::from("/tmp/logs/build-run-complete.log"),
                    artifact_root_path: String::from("/tmp/artifacts/build-run-complete"),
                },
            )
            .expect("build run should start");
        assert_eq!(running.status, BuildStatus::Running.as_str());
        assert_eq!(
            running.workspace_path.as_deref(),
            Some("/tmp/runs/build-run-complete")
        );
        assert!(running.started_at.is_some());
        assert!(running.finished_at.is_none());
        assert!(running.error_message.is_none());

        let release_after_start = coordinator
            .load_release_run_record(fixture.release_run_id)
            .expect("started release should reload")
            .expect("started release should exist");
        assert_eq!(release_after_start.status, ReleaseStatus::Running.as_str());
        assert_eq!(release_after_start.started_at, running.started_at);
        assert!(release_after_start.finished_at.is_none());

        let completed = coordinator
            .complete_build_run(
                fixture.build_run_id,
                CompleteBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                },
            )
            .expect("build run should complete");
        assert_eq!(completed.status, BuildStatus::Succeeded.as_str());
        assert_eq!(
            completed.log_path.as_deref(),
            Some("/tmp/logs/build-run-complete.log")
        );
        assert_eq!(
            completed.artifact_root_path.as_deref(),
            Some("/tmp/artifacts/build-run-complete")
        );
        assert!(completed.finished_at.is_some());
        assert!(completed.error_message.is_none());

        let release_after_complete = coordinator
            .load_release_run_record(fixture.release_run_id)
            .expect("completed release should reload")
            .expect("completed release should exist");
        assert_eq!(release_after_complete.status, ReleaseStatus::Succeeded.as_str());
        assert_eq!(release_after_complete.finished_at, completed.finished_at);
        assert!(release_after_complete.error_message.is_none());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_process_feed_page_reconciles_stale_release_status_from_terminal_builds() {
        let root = test_root("process-feed-release-reconciliation");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "process-feed-reconcile");
        let release_run_id = insert_release_run(
            &connection,
            fixture.repository_id,
            "v2.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            fixture.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "
                UPDATE build_runs
                SET started_at = ?,
                    finished_at = ?,
                    current_stage_label = ?,
                    current_stage_status = ?,
                    last_progress_message = ?
                WHERE id = ?
                ",
                params![
                    "2026-01-11T14:00:00Z",
                    "2026-01-11T14:07:00Z",
                    "Register Artifacts",
                    BuildStatus::Succeeded.as_str(),
                    "Artifacts registered and downstream publish work dispatched.",
                    build_run_id,
                ],
            )
            .expect("completed build metadata should update");
        drop(connection);

        let page = list_process_feed_page(&layout, 1, 10)
            .expect("process feed page should reconcile stale release rows");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].release_run_id, release_run_id);
        assert_eq!(page.items[0].display_status, "succeeded");
        assert_eq!(page.items[0].current_step_label, "Built Windows");
        assert_eq!(page.items[0].current_step_status, "succeeded");

        let coordinator = LocalCoordinator::new(&layout);
        let release = coordinator
            .load_release_run_record(release_run_id)
            .expect("reconciled release should reload")
            .expect("reconciled release should exist");
        assert_eq!(release.status, ReleaseStatus::Succeeded.as_str());
        assert_eq!(release.started_at.as_deref(), Some("2026-01-11T14:00:00Z"));
        assert_eq!(release.finished_at.as_deref(), Some("2026-01-11T14:07:00Z"));

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_polling_repositories_excludes_local_workspaces() {
        let root = test_root("polling-repositories-managed-only");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let managed = seed_repository_fixture(&connection, "managed-poll");
        connection
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    source_mode,
                    workspace_strategy,
                    repo_url,
                    local_path,
                    polling_interval_seconds,
                    enabled
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    "local-direct",
                    "local_workspace",
                    "direct",
                    Option::<String>::None,
                    String::from("C:/Users/gabao/projects/Games/revolutions"),
                    0,
                    1,
                ],
            )
            .expect("local workspace repository should insert");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let repositories = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load");

        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].id, managed.repository_id);
        assert_eq!(repositories[0].name, "managed-poll");
        assert_eq!(repositories[0].repo_url, "https://example.com/managed-poll.git");
        assert!(!repositories[0].has_release_history);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_repository_projects_includes_local_workspaces() {
        let root = test_root("repository-projects-include-local-workspaces");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let managed = seed_repository_fixture(&connection, "managed-project");
        connection
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    source_mode,
                    workspace_strategy,
                    repo_url,
                    local_path,
                    polling_interval_seconds,
                    enabled,
                    engine_kind
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    "local-project",
                    "local_workspace",
                    "direct",
                    Option::<String>::None,
                    String::from("C:/Users/gabao/projects/Games/revolutions"),
                    0,
                    1,
                    "unity",
                ],
            )
            .expect("local workspace repository should insert");
        let local_repository_id = connection.last_insert_rowid();
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let repositories = coordinator
            .list_repository_projects()
            .expect("repository projects should load");

        assert_eq!(repositories.len(), 2);

        let managed_record = repositories
            .iter()
            .find(|repository| repository.id == managed.repository_id)
            .expect("managed repository should be listed");
        assert_eq!(managed_record.source_mode, "managed_repository");
        assert_eq!(
            managed_record.repo_url.as_deref(),
            Some("https://example.com/managed-project.git")
        );
        assert_eq!(managed_record.local_path, None);

        let local_record = repositories
            .iter()
            .find(|repository| repository.id == local_repository_id)
            .expect("local workspace repository should be listed");
        assert_eq!(local_record.name, "local-project");
        assert_eq!(local_record.source_mode, "local_workspace");
        assert_eq!(local_record.workspace_strategy, "direct");
        assert_eq!(local_record.repo_url, None);
        assert_eq!(
            local_record.local_path.as_deref(),
            Some("C:/Users/gabao/projects/Games/revolutions")
        );
        assert_eq!(local_record.polling_interval_seconds, 0);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn list_polling_repositories_reports_release_history_presence() {
        let root = test_root("polling-repositories-history");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_repository_fixture(&connection, "managed-poll-history");
        insert_release_run(
            &connection,
            fixture.repository_id,
            "v1.2.0",
            ReleaseStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let repositories = coordinator
            .list_polling_repositories()
            .expect("managed polling repositories should load");

        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].id, fixture.repository_id);
        assert!(repositories[0].has_release_history);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn get_repository_checkout_record_loads_managed_repository_metadata() {
        let root = test_root("repository-checkout-record");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        connection
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                VALUES (?, ?, ?)
                ",
                params![
                    "managed-origin",
                    "git-http-basic",
                    r#"{"username":"worker","password":"secret"}"#,
                ],
            )
            .expect("credentials should insert");
        let credentials_id = connection.last_insert_rowid();

        let repository = seed_repository_fixture(&connection, "managed-checkout");
        connection
            .execute(
                "
                UPDATE repositories
                SET credentials_id = ?,
                    default_branch = ?,
                    workspace_root_override = ?
                WHERE id = ?
                ",
                params![
                    credentials_id,
                    "main",
                    "D:/runtime/repositories/managed-checkout",
                    repository.repository_id,
                ],
            )
            .expect("repository checkout metadata should update");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let record = coordinator
            .get_repository_checkout_record(repository.repository_id)
            .expect("repository checkout record should load");

        assert_eq!(record.id, repository.repository_id);
        assert_eq!(record.name, "managed-checkout");
        assert_eq!(record.source_mode, "managed_repository");
        assert_eq!(record.workspace_strategy, "managed_checkout");
        assert_eq!(
            record.repo_url.as_deref(),
            Some("https://example.com/managed-checkout.git")
        );
        assert_eq!(record.credentials_id, Some(credentials_id));
        assert_eq!(record.default_branch.as_deref(), Some("main"));
        assert_eq!(
            record.workspace_root_override.as_deref(),
            Some("D:/runtime/repositories/managed-checkout")
        );
        assert!(record.enabled);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn import_repository_registration_from_database_copies_configuration_without_runs() {
        let root = test_root("import-repository-registration");
        let source_layout = StorageLayout::from_directories(&RuntimeDirectories::from_root(
            root.join("source-runtime"),
        ));
        let target_layout = StorageLayout::from_directories(&RuntimeDirectories::from_root(
            root.join("target-runtime"),
        ));
        initialize_database(&source_layout).expect("source database bootstrap should succeed");
        initialize_database(&target_layout).expect("target database bootstrap should succeed");

        let source = open_connection(&source_layout.database_path).expect("source connection should open");
        source
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                VALUES (?, ?, ?)
                ",
                params![
                    "revolutions/origin",
                    "git-http-basic",
                    r#"{"username":"comrade","password":"sickle"}"#,
                ],
            )
            .expect("source repository credentials should insert");
        let repository_credentials_id = source.last_insert_rowid();
        source
            .execute(
                "
                INSERT INTO credentials (name, kind, config_json)
                VALUES (?, ?, ?)
                ",
                params![
                    "revolutions/publish",
                    "git-http-basic",
                    r#"{"username":"builder","password":"hammer"}"#,
                ],
            )
            .expect("source publish credentials should insert");
        let publish_credentials_id = source.last_insert_rowid();
        source
            .execute(
                "
                INSERT INTO repositories (
                    name,
                    source_mode,
                    workspace_strategy,
                    repo_url,
                    credentials_id,
                    default_branch,
                    artifacts_root_override,
                    workspace_root_override,
                    polling_interval_seconds,
                    enabled
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    "Revolutions",
                    "managed_repository",
                    "managed_checkout",
                    "https://example.com/revolutions.git",
                    repository_credentials_id,
                    "main",
                    "D:/build-output",
                    "D:/managed-workspace",
                    300,
                    1,
                ],
            )
            .expect("source repository should insert");
        let repository_id = source.last_insert_rowid();
        source
            .execute(
                "
                INSERT INTO trigger_rules (repository_id, name, source, enabled, config_json)
                VALUES (?, ?, ?, ?, ?)
                ",
                params![repository_id, "poll-default", "poll", 1, r#"{"interval":300}"#],
            )
            .expect("source trigger rule should insert");
        source
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    runner_type,
                    enabled,
                    contract_json,
                    config_json
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    "host-native",
                    1,
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.BuildWindows",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                    "{}",
                ],
            )
            .expect("source build target should insert");
        let build_target_id = source.last_insert_rowid();
        source
            .execute(
                "
                INSERT INTO publish_targets (
                    repository_id,
                    name,
                    kind,
                    credentials_id,
                    enabled,
                    config_json
                ) VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "filesystem-release",
                    "filesystem",
                    publish_credentials_id,
                    1,
                    "{}",
                ],
            )
            .expect("source publish target should insert");
        let publish_target_id = source.last_insert_rowid();
        source
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
                    publish_target_id,
                    1,
                    r#"{"operation":"move","directory_path":"D:/exports"}"#,
                ],
            )
            .expect("source binding should insert");
        source
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
        drop(source);

        let coordinator = LocalCoordinator::new(&target_layout);
        let report = coordinator
            .import_repository_registration_from_database(
                &source_layout.database_path,
                "Revolutions",
            )
            .expect("repository registration should import");

        assert_eq!(report.repository_name, "Revolutions");
        assert_eq!(report.credential_name.as_deref(), Some("revolutions/origin"));
        assert_eq!(report.trigger_rule_count, 1);
        assert_eq!(report.build_target_count, 1);
        assert_eq!(report.publish_target_count, 1);
        assert_eq!(report.binding_count, 1);

        let target = open_connection(&target_layout.database_path).expect("target connection should open");
        let repository_row: (Option<String>, Option<String>, Option<String>) = target
            .query_row(
                "
                SELECT default_branch,
                       artifacts_root_override,
                       workspace_root_override
                FROM repositories
                WHERE name = ?
                ",
                ["Revolutions"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("imported repository should load");
        assert_eq!(repository_row.0.as_deref(), Some("main"));
        assert_eq!(repository_row.1.as_deref(), Some("D:/build-output"));
        assert_eq!(repository_row.2.as_deref(), Some("D:/managed-workspace"));

        let release_run_count: i64 = target
            .query_row(
                "
                SELECT COUNT(1)
                FROM release_runs rr
                JOIN repositories r ON r.id = rr.repository_id
                WHERE r.name = ?
                ",
                ["Revolutions"],
                |row| row.get(0),
            )
            .expect("imported release run count should load");
        assert_eq!(release_run_count, 0);
        drop(target);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn fail_build_run_requires_running_state_and_error_message() {
        let root = test_root("build-run-fail");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "build-run-fail");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let empty_error = coordinator
            .fail_build_run(
                fixture.build_run_id,
                FailBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                    error_message: String::new(),
                },
            )
            .expect_err("empty error message should be rejected");
        assert_eq!(empty_error.kind(), std::io::ErrorKind::InvalidInput);

        coordinator
            .start_build_run(
                fixture.build_run_id,
                StartBuildRunInput {
                    workspace_path: String::from("/tmp/runs/build-run-fail"),
                    log_path: String::from("/tmp/logs/build-run-fail.log"),
                    artifact_root_path: String::from("/tmp/artifacts/build-run-fail"),
                },
            )
            .expect("build run should start");
        let failed = coordinator
            .fail_build_run(
                fixture.build_run_id,
                FailBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                    error_message: String::from("runner exploded"),
                },
            )
            .expect("build run should fail");
        assert_eq!(failed.status, BuildStatus::Failed.as_str());
        assert_eq!(failed.error_message.as_deref(), Some("runner exploded"));
        assert!(failed.finished_at.is_some());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn cancel_build_run_requires_running_state_and_error_message() {
        let root = test_root("build-run-cancel");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "build-run-cancel");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let empty_error = coordinator
            .cancel_build_run(
                fixture.build_run_id,
                CancelBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                    error_message: String::new(),
                },
            )
            .expect_err("empty cancel message should be rejected");
        assert_eq!(empty_error.kind(), std::io::ErrorKind::InvalidInput);

        coordinator
            .start_build_run(
                fixture.build_run_id,
                StartBuildRunInput {
                    workspace_path: String::from("/tmp/runs/build-run-cancel"),
                    log_path: String::from("/tmp/logs/build-run-cancel.log"),
                    artifact_root_path: String::from("/tmp/artifacts/build-run-cancel"),
                },
            )
            .expect("build run should start");
        let canceled = coordinator
            .cancel_build_run(
                fixture.build_run_id,
                CancelBuildRunInput {
                    workspace_path: String::new(),
                    log_path: String::new(),
                    artifact_root_path: String::new(),
                    error_message: String::from("timeout: host-native unity runner exceeded 1s timeout"),
                },
            )
            .expect("build run should cancel");
        assert_eq!(canceled.status, BuildStatus::Canceled.as_str());
        assert_eq!(
            canceled.error_message.as_deref(),
            Some("timeout: host-native unity runner exceeded 1s timeout")
        );
        assert!(canceled.finished_at.is_some());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn cancel_release_run_cancels_running_and_queued_children_without_dispatching_next_build() {
        let root = test_root("release-run-cancel-all");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "release-cancel-all-repo");
        let release_run_id =
            insert_release_run(&connection, repo.repository_id, "v8.0.0", ReleaseStatus::Running.as_str());
        let running_build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        let queued_build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.secondary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        let artifact_id = insert_artifact(&connection, running_build_run_id, "release.zip");
        let queued_publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            running_build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let canceled_release = coordinator
            .cancel_release_run(
                release_run_id,
                CancelReleaseRunInput {
                    error_message: String::from(
                        "Canceled by operator while process was on hold.",
                    ),
                },
            )
            .expect("release run should cancel");

        assert_eq!(canceled_release.status, ReleaseStatus::Canceled.as_str());
        assert!(canceled_release.finished_at.is_some());
        assert_eq!(
            canceled_release.error_message.as_deref(),
            Some("Canceled by operator while process was on hold."),
        );

        let running_build = coordinator
            .get_build_run_record(running_build_run_id)
            .expect("running build should load");
        assert_eq!(running_build.status, BuildStatus::Canceled.as_str());
        assert!(running_build.finished_at.is_some());

        let queued_build = coordinator
            .get_build_run_record(queued_build_run_id)
            .expect("queued build should load");
        assert_eq!(queued_build.status, BuildStatus::Canceled.as_str());
        assert!(queued_build.finished_at.is_some());

        let publish = coordinator
            .get_publish_run_record(queued_publish_run_id)
            .expect("queued publish should load");
        assert_eq!(publish.status, PublishStatus::Canceled.as_str());
        assert!(publish.finished_at.is_some());

        assert_eq!(queue_message_count(&layout.database_path, BUILD_RUN_QUEUE_NAME), 0);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn replace_build_artifacts_replaces_existing_rows_for_one_run() {
        let root = test_root("replace-build-artifacts");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "replace-artifacts");
        insert_artifact(&connection, fixture.build_run_id, "old.zip");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let artifacts = coordinator
            .replace_build_artifacts(
                fixture.build_run_id,
                vec![
                    CreateArtifactRecordInput {
                        name: String::from("release.zip"),
                        kind: String::from("archive"),
                        path: String::from("release.zip"),
                        size_bytes: Some(128),
                        checksum_sha256: None,
                    },
                    CreateArtifactRecordInput {
                        name: String::from("notes.txt"),
                        kind: String::from("file"),
                        path: String::from("notes.txt"),
                        size_bytes: Some(12),
                        checksum_sha256: Some(String::from("abc123")),
                    },
                ],
            )
            .expect("artifact replacement should succeed");

        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].path, "release.zip");
        assert_eq!(artifacts[1].checksum_sha256.as_deref(), Some("abc123"));

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let artifact_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM artifacts WHERE build_run_id = ?",
                [fixture.build_run_id],
                |row| row.get(0),
            )
            .expect("artifact count should load");
        let old_path_exists: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM artifacts WHERE build_run_id = ? AND path = ?",
                params![fixture.build_run_id, "artifacts/old.zip"],
                |row| row.get(0),
            )
            .expect("replaced artifact should disappear");
        assert_eq!(artifact_count, 2);
        assert_eq!(old_path_exists, 0);
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_build_publish_runs_creates_one_queued_run_per_binding_and_artifact() {
        let root = test_root("plan-build-publish-runs");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-plan-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v7.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        let first_artifact_id = insert_artifact(&connection, build_run_id, "release.zip");
        let second_artifact_id = insert_artifact(&connection, build_run_id, "notes.txt");
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            repo.publish_target_id,
            true,
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect("publish planning should succeed");

        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|run| run.status == PublishStatus::Queued.as_str()));
        assert_eq!(runs[0].artifact_id, Some(first_artifact_id));
        assert_eq!(runs[1].artifact_id, Some(second_artifact_id));

        let rerun = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect("duplicate publish planning should stay idempotent");
        assert_eq!(rerun.len(), 2);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_build_publish_runs_orders_non_consuming_before_consuming_for_same_artifact() {
        let root = test_root("plan-build-publish-run-order");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-order-repo");
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            repo.publish_target_id,
            true,
        );
        connection
            .execute(
                "UPDATE build_publish_bindings SET options_json = ? WHERE build_target_id = ? AND publish_target_id = ?",
                params![
                    serde_json::json!({
                        "operation": "move",
                        "directory_path": "D:/Published/Primary",
                    })
                    .to_string(),
                    repo.primary_build_target_id,
                    repo.publish_target_id,
                ],
            )
            .expect("filesystem binding should update");
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repo.repository_id, "itch-release", "itch"],
            )
            .expect("itch publish target should insert");
        let itch_publish_target_id = connection.last_insert_rowid();
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            itch_publish_target_id,
            true,
        );

        let release_run_id = insert_release_run(&connection, repo.repository_id, "v8.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        insert_artifact(&connection, build_run_id, "game.zip");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect("publish planning should succeed");

        assert_eq!(runs.len(), 2);
        let first_snapshot: Value = serde_json::from_str(&runs[0].execution_contract_json)
            .expect("first execution snapshot should decode");
        let second_snapshot: Value = serde_json::from_str(&runs[1].execution_contract_json)
            .expect("second execution snapshot should decode");
        assert_eq!(first_snapshot["publish_target_kind"], "itch");
        assert_eq!(first_snapshot["consumption_behavior"], "non_consuming");
        assert_eq!(second_snapshot["publish_target_kind"], "filesystem");
        assert_eq!(second_snapshot["consumption_behavior"], "consuming");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_build_publish_runs_rejects_multiple_consuming_bindings_for_build_target() {
        let root = test_root("plan-build-publish-multiple-consuming");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-consuming-repo");
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            repo.publish_target_id,
            true,
        );
        connection
            .execute(
                "UPDATE build_publish_bindings SET options_json = ? WHERE build_target_id = ? AND publish_target_id = ?",
                params![
                    serde_json::json!({
                        "operation": "move",
                        "directory_path": "D:/Published/Primary",
                    })
                    .to_string(),
                    repo.primary_build_target_id,
                    repo.publish_target_id,
                ],
            )
            .expect("first consuming binding should update");
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repo.repository_id, "second-filesystem", "filesystem"],
            )
            .expect("second filesystem target should insert");
        let second_publish_target_id = connection.last_insert_rowid();
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            second_publish_target_id,
            true,
        );
        connection
            .execute(
                "UPDATE build_publish_bindings SET options_json = ? WHERE build_target_id = ? AND publish_target_id = ?",
                params![
                    serde_json::json!({
                        "operation": "move",
                        "directory_path": "E:/Published/Secondary",
                    })
                    .to_string(),
                    repo.primary_build_target_id,
                    second_publish_target_id,
                ],
            )
            .expect("second consuming binding should update");

        let release_run_id = insert_release_run(&connection, repo.repository_id, "v8.1.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        insert_artifact(&connection, build_run_id, "game.zip");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let error = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect_err("multiple consuming bindings should be rejected");
        assert!(error
            .to_string()
            .contains("cannot enable more than one consuming publish binding"));

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn publish_execution_plan_and_completion_load_artifact_source_path() {
        let root = test_root("publish-execution-plan");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let artifact_root = root.join("artifacts").join("revolutions.v9.0.0");
        std::fs::create_dir_all(artifact_root.join("nested"))
            .expect("artifact root should create");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-plan-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v9.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "UPDATE build_runs SET artifact_root_path = ? WHERE id = ?",
                params![artifact_root.display().to_string(), build_run_id],
            )
            .expect("artifact root path should update");
        let artifact_id = insert_artifact(&connection, build_run_id, "nested/game.zip");
        let publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let plan = coordinator
            .get_publish_execution_plan(publish_run_id)
            .expect("publish execution plan should load");
        assert_eq!(plan.publish_target_kind, "filesystem");
        assert_eq!(plan.artifact_path, "artifacts/nested/game.zip");
        assert_eq!(plan.artifact_active_location_kind, "runtime_artifact");
        assert_eq!(plan.artifact_active_location_ref, "artifacts/nested/game.zip");
        assert_eq!(plan.execution_contract_json, "{}");
        assert!(plan.source_path.ends_with("nested\\game.zip") || plan.source_path.ends_with("nested/game.zip"));

        let running = coordinator
            .start_publish_run(publish_run_id, StartPublishRunInput::default())
            .expect("publish run should start");
        assert_eq!(running.status, PublishStatus::Running.as_str());

        let release_after_publish_start = coordinator
            .load_release_run_record(release_run_id)
            .expect("publish-started release should reload")
            .expect("publish-started release should exist");
        assert_eq!(release_after_publish_start.status, ReleaseStatus::Running.as_str());

        let completed = coordinator
            .complete_publish_run(
                publish_run_id,
                CompletePublishRunInput {
                    destination_ref: String::from("C:/published/revolutions/v9.0.0/nested/game.zip"),
                    artifact_active_location_kind: None,
                    artifact_active_location_ref: None,
                },
            )
            .expect("publish run should complete");
        assert_eq!(completed.status, PublishStatus::Succeeded.as_str());
        assert_eq!(
            completed.destination_ref.as_deref(),
            Some("C:/published/revolutions/v9.0.0/nested/game.zip")
        );

        let release_after_publish_complete = coordinator
            .load_release_run_record(release_run_id)
            .expect("completed publish release should reload")
            .expect("completed publish release should exist");
        assert_eq!(
            release_after_publish_complete.status,
            ReleaseStatus::Succeeded.as_str()
        );
        assert!(release_after_publish_complete.finished_at.is_some());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn complete_publish_run_updates_artifact_active_location_when_provided() {
        let root = test_root("complete-publish-run-active-location");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "complete-publish-active-location");
        let artifact_id = insert_artifact(
            &connection,
            fixture.build_run_id,
            "players/game.zip",
        );
        let publish_target_id: i64 = connection
            .query_row(
                "SELECT id FROM publish_targets WHERE repository_id = ? ORDER BY id ASC LIMIT 1",
                [fixture.repository_id],
                |row| row.get(0),
            )
            .expect("publish target should load");
        let publish_run_id = insert_publish_run(
            &connection,
            fixture.release_run_id,
            fixture.build_run_id,
            publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        coordinator
            .start_publish_run(publish_run_id, StartPublishRunInput::default())
            .expect("publish run should start");
        coordinator
            .complete_publish_run(
                publish_run_id,
                CompletePublishRunInput {
                    destination_ref: String::from("C:/published/revolutions/v10.0.0/game.zip"),
                    artifact_active_location_kind: Some(String::from("filesystem_absolute")),
                    artifact_active_location_ref: Some(String::from(
                        "C:/published/revolutions/v10.0.0/game.zip",
                    )),
                },
            )
            .expect("publish run should complete with active location update");

        let artifacts = coordinator
            .list_artifacts_by_build_run(fixture.build_run_id)
            .expect("artifacts should reload");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].active_location_kind, "filesystem_absolute");
        assert_eq!(
            artifacts[0].active_location_ref,
            "C:/published/revolutions/v10.0.0/game.zip"
        );

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn replace_build_artifacts_sets_runtime_active_location_defaults() {
        let root = test_root("artifact-active-location-defaults");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let fixture = seed_build_execution_fixture(&connection, "active-location-defaults");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let artifacts = coordinator
            .replace_build_artifacts(
                fixture.build_run_id,
                vec![CreateArtifactRecordInput {
                    name: String::from("release.zip"),
                    kind: String::from("archive"),
                    path: String::from("releases/release.zip"),
                    size_bytes: Some(128),
                    checksum_sha256: None,
                }],
            )
            .expect("artifact replacement should succeed");

        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].active_location_kind, "runtime_artifact");
        assert_eq!(artifacts[0].active_location_ref, "releases/release.zip");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn plan_build_publish_runs_persists_execution_contract_snapshot() {
        let root = test_root("publish-run-execution-contract");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-contract-repo");
        connection
            .execute(
                "UPDATE publish_targets SET config_json = ? WHERE id = ?",
                params![
                    serde_json::json!({"directory_path": "D:/Published/Windows"}).to_string(),
                    repo.publish_target_id,
                ],
            )
            .expect("publish target config should update");
        connection
            .execute(
                "INSERT INTO build_publish_bindings (build_target_id, publish_target_id, enabled, options_json) VALUES (?, ?, ?, ?)",
                params![
                    repo.primary_build_target_id,
                    repo.publish_target_id,
                    1,
                    "{}",
                ],
            )
            .expect("build publish binding should insert");
        connection
            .execute(
                "UPDATE build_publish_bindings SET options_json = ? WHERE build_target_id = ? AND publish_target_id = ?",
                params![
                    serde_json::json!({
                        "operation": "move",
                        "directory_path": "D:/Published/Windows",
                    })
                    .to_string(),
                    repo.primary_build_target_id,
                    repo.publish_target_id,
                ],
            )
            .expect("binding options should update");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v1.2.3", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        let artifact_id = insert_artifact(&connection, build_run_id, "game.zip");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let runs = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect("publish planning should succeed");

        assert_eq!(runs.len(), 1);
        let snapshot: serde_json::Value = serde_json::from_str(&runs[0].execution_contract_json)
            .expect("execution contract snapshot should decode");
        assert_eq!(snapshot["publish_target_id"], repo.publish_target_id);
        assert_eq!(snapshot["artifact_id"], artifact_id);
        assert_eq!(snapshot["publish_target_kind"], "filesystem");
        assert_eq!(
            snapshot["binding_options_json"],
            serde_json::json!({
                "operation": "move",
                "directory_path": "D:/Published/Windows",
            })
            .to_string()
        );
        assert_eq!(snapshot["artifact_active_location_kind"], "runtime_artifact");
        assert_eq!(snapshot["artifact_active_location_ref"], "artifacts/game.zip");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn get_publish_execution_plan_uses_snapshotted_target_metadata_after_destination_edit() {
        let root = test_root("publish-run-snapshot-after-edit");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-snapshot-edit");
        connection
            .execute(
                "UPDATE publish_targets SET name = ? WHERE id = ?",
                params!["windows-original", repo.publish_target_id],
            )
            .expect("original publish target metadata should persist");
        insert_build_publish_binding(
            &connection,
            repo.primary_build_target_id,
            repo.publish_target_id,
            true,
        );
        let release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v1.2.4",
            ReleaseStatus::Queued.as_str(),
        );
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "UPDATE build_runs SET artifact_root_path = ? WHERE id = ?",
                params!["C:/artifacts/publish-snapshot-edit", build_run_id],
            )
            .expect("build artifact root should persist");
        insert_artifact(&connection, build_run_id, "game.zip");
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let publish_runs = coordinator
            .plan_build_publish_runs(build_run_id)
            .expect("publish planning should succeed");
        assert_eq!(publish_runs.len(), 1);

        let connection = open_connection(&layout.database_path).expect("connection should reopen");
        connection
            .execute(
                "UPDATE publish_targets SET name = ? WHERE id = ?",
                params!["windows-edited", repo.publish_target_id],
            )
            .expect("edited publish target metadata should persist");
        drop(connection);

        let plan = coordinator
            .get_publish_execution_plan(publish_runs[0].id)
            .expect("publish execution plan should load");
        assert_eq!(plan.publish_target_name, "windows-original");
        assert_eq!(plan.publish_target_kind, "filesystem");
        assert_eq!(plan.publish_target_config_json, "{}");

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn publish_execution_plan_prefers_active_absolute_location() {
        let root = test_root("publish-execution-active-location");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let moved_root = root.join("published");
        std::fs::create_dir_all(&moved_root).expect("published root should create");
        let moved_path = moved_root.join("game.zip");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-active-location-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v9.0.1", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        connection
            .execute(
                "UPDATE build_runs SET artifact_root_path = ? WHERE id = ?",
                params![root.join("artifacts").display().to_string(), build_run_id],
            )
            .expect("artifact root path should update");
        let artifact_id = insert_artifact(&connection, build_run_id, "game.zip");
        connection
            .execute(
                "UPDATE artifacts SET active_location_kind = ?, active_location_ref = ? WHERE id = ?",
                params![
                    "filesystem_absolute",
                    moved_path.display().to_string(),
                    artifact_id,
                ],
            )
            .expect("artifact active location should update");
        let publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let plan = coordinator
            .get_publish_execution_plan(publish_run_id)
            .expect("publish execution plan should load");

        assert_eq!(plan.artifact_active_location_kind, "filesystem_absolute");
        assert_eq!(plan.artifact_active_location_ref, moved_path.display().to_string());
        assert_eq!(plan.source_path, moved_path.display().to_string());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn build_job_claim_skips_stale_and_blocked_release_messages() {
        let root = test_root("build-claim-lane");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "repo-a");
        let stale_release = insert_release_run(&connection, repo.repository_id, "v1.0.0", "queued");
        let stale_run = insert_build_run(
            &connection,
            stale_release,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );

        let active_release = insert_release_run(&connection, repo.repository_id, "v1.1.0", "queued");
        insert_build_run(
            &connection,
            active_release,
            repo.primary_build_target_id,
            BuildStatus::Running.as_str(),
        );
        let eligible_run = insert_build_run(
            &connection,
            active_release,
            repo.secondary_build_target_id,
            BuildStatus::Queued.as_str(),
        );

        let blocked_release = insert_release_run(&connection, repo.repository_id, "v1.2.0", "queued");
        let blocked_run = insert_build_run(
            &connection,
            blocked_release,
            repo.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );

        let stale_message_id = enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, stale_run).as_bytes(),
        );
        let blocked_message_id = enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, blocked_run).as_bytes(),
        );
        let eligible_message_id = enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, eligible_run).as_bytes(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let limits = RuntimeConcurrencySettings {
            max_concurrent_build_runs: 2,
            max_concurrent_publish_runs: 1,
            max_active_releases_per_repository: 1,
        };
        let claimed = coordinator
            .claim_next_build_job(
                "worker-a",
                Duration::ZERO,
                Duration::from_millis(40),
                &limits,
            )
            .expect("build claim should succeed");
        assert!(claimed.is_none());

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert!(!queue_message_exists(&connection, stale_message_id));
        assert!(queue_message_exists(&connection, blocked_message_id));
        let blocked_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [blocked_message_id],
                |row| row.get(0),
            )
            .expect("blocked queue message should load");
        assert!(blocked_leased_by.is_none());
        assert!(queue_message_exists(&connection, eligible_message_id));
        let eligible_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [eligible_message_id],
                |row| row.get(0),
            )
            .expect("eligible queue message should load");
        assert!(eligible_leased_by.is_none());
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn build_job_claim_preserves_repository_release_order_when_messages_are_out_of_order() {
        let root = test_root("build-claim-release-order");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "repo-order");
        let first_release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v2.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        let first_run_id = insert_build_run(
            &connection,
            first_release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );

        let second_release_run_id = insert_release_run(
            &connection,
            repo.repository_id,
            "v2.1.0",
            ReleaseStatus::Queued.as_str(),
        );
        let second_run_id = insert_build_run(
            &connection,
            second_release_run_id,
            repo.secondary_build_target_id,
            BuildStatus::Queued.as_str(),
        );

        let newer_message_id = enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, second_run_id).as_bytes(),
        );
        let older_message_id = enqueue_message(
            &connection,
            BUILD_RUN_QUEUE_NAME,
            format!(r#"{{"build_run_id":{}}}"#, first_run_id).as_bytes(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let limits = RuntimeConcurrencySettings {
            max_concurrent_build_runs: 1,
            max_concurrent_publish_runs: 1,
            max_active_releases_per_repository: 1,
        };
        let claimed = coordinator
            .claim_next_build_job(
                "worker-order",
                Duration::ZERO,
                Duration::from_millis(40),
                &limits,
            )
            .expect("build claim should succeed")
            .expect("one queued build job should be claimable");
        assert_eq!(claimed.id, older_message_id);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let newer_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [newer_message_id],
                |row| row.get(0),
            )
            .expect("newer queue message should load");
        let older_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [older_message_id],
                |row| row.get(0),
            )
            .expect("older queue message should load");
        assert!(newer_leased_by.is_none());
        assert_eq!(older_leased_by.as_deref(), Some("worker-order"));
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn publish_job_claim_skips_stale_messages_and_claims_valid_work() {
        let root = test_root("publish-claim");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v2.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        let artifact_id = insert_artifact(&connection, build_run_id, "game.zip");
        let stale_publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Failed.as_str(),
        );
        let valid_publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        let stale_message_id = enqueue_message(
            &connection,
            PUBLISH_RUN_QUEUE_NAME,
            format!(r#"{{"publish_run_id":{}}}"#, stale_publish_run_id).as_bytes(),
        );
        let valid_message_id = enqueue_message(
            &connection,
            PUBLISH_RUN_QUEUE_NAME,
            format!(r#"{{"publish_run_id":{}}}"#, valid_publish_run_id).as_bytes(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let claimed = coordinator
            .claim_next_publish_job(
                "publisher-a",
                Duration::ZERO,
                Duration::from_millis(40),
                &RuntimeConcurrencySettings::development(),
            )
            .expect("publish claim should succeed")
            .expect("valid publish job should be claimed");
        assert_eq!(claimed.id, valid_message_id);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        assert!(!queue_message_exists(&connection, stale_message_id));
        assert!(queue_message_exists(&connection, valid_message_id));
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn publish_job_claim_preserves_artifact_order_when_messages_are_out_of_order() {
        let root = test_root("publish-claim-artifact-order");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "publish-claim-order-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v2.1.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        let artifact_id = insert_artifact(&connection, build_run_id, "game.zip");
        let first_publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repo.repository_id, "itch-release", "itch"],
            )
            .expect("itch publish target should insert");
        let second_publish_target_id = connection.last_insert_rowid();
        let second_publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            second_publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );

        let newer_message_id = enqueue_message(
            &connection,
            PUBLISH_RUN_QUEUE_NAME,
            format!(r#"{{"publish_run_id":{}}}"#, second_publish_run_id).as_bytes(),
        );
        let older_message_id = enqueue_message(
            &connection,
            PUBLISH_RUN_QUEUE_NAME,
            format!(r#"{{"publish_run_id":{}}}"#, first_publish_run_id).as_bytes(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        let claimed = coordinator
            .claim_next_publish_job(
                "publisher-order",
                Duration::ZERO,
                Duration::from_millis(40),
                &RuntimeConcurrencySettings::development(),
            )
            .expect("publish claim should succeed")
            .expect("older publish run should be claimed first");
        assert_eq!(claimed.id, older_message_id);

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let newer_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [newer_message_id],
                |row| row.get(0),
            )
            .expect("newer queue message should load");
        let older_leased_by: Option<String> = connection
            .query_row(
                "SELECT leased_by FROM worker_queue_messages WHERE id = ?",
                [older_message_id],
                |row| row.get(0),
            )
            .expect("older queue message should load");
        assert!(newer_leased_by.is_none());
        assert_eq!(older_leased_by.as_deref(), Some("publisher-order"));
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn publish_run_dispatch_enqueues_compatible_payload_and_is_idempotent() {
        let root = test_root("publish-dispatch");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let repo = seed_repository_fixture(&connection, "dispatch-publish-repo");
        let release_run_id = insert_release_run(&connection, repo.repository_id, "v4.0.0", "queued");
        let build_run_id = insert_build_run(
            &connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Succeeded.as_str(),
        );
        let artifact_id = insert_artifact(&connection, build_run_id, "release.zip");
        let publish_run_id = insert_publish_run(
            &connection,
            release_run_id,
            build_run_id,
            repo.publish_target_id,
            artifact_id,
            PublishStatus::Queued.as_str(),
        );
        drop(connection);

        let coordinator = LocalCoordinator::new(&layout);
        assert_eq!(
            coordinator
                .dispatch_publish_run(publish_run_id)
                .expect("first publish dispatch should succeed"),
            QueueDispatchOutcome::Enqueued,
        );
        assert_eq!(
            coordinator
                .dispatch_publish_run(publish_run_id)
                .expect("duplicate publish dispatch should succeed"),
            QueueDispatchOutcome::AlreadyClaimed,
        );

        let claimed = coordinator
            .claim_next_publish_job(
                "publish-worker-a",
                Duration::ZERO,
                Duration::from_millis(40),
                &RuntimeConcurrencySettings::development(),
            )
            .expect("publish claim should succeed")
            .expect("queued publish job should be claimable");
        let decoded: Value =
            serde_json::from_slice(&claimed.payload).expect("payload should decode as JSON");
        assert_eq!(decoded["publish_run_id"], publish_run_id);
        assert_eq!(decoded["release_run_id"], release_run_id);
        assert_eq!(decoded["build_run_id"], build_run_id);
        assert_eq!(decoded["publish_target_id"], repo.publish_target_id);
        assert_eq!(decoded["artifact_id"], artifact_id);

        let job: PublishDispatchJob = serde_json::from_slice(&claimed.payload)
            .expect("payload should match publish job contract");
        assert_eq!(job.publish_run_id, publish_run_id);
        assert_eq!(job.artifact_id, Some(artifact_id));
        assert_eq!(queue_message_count(&layout.database_path, PUBLISH_RUN_QUEUE_NAME), 1);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn local_locks_respect_renew_release_and_expiration() {
        let root = test_root("locks");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        let first = coordinator
            .acquire_lock("release:v1.2.3", Duration::from_millis(5_000))
            .expect("lock acquisition should succeed")
            .expect("lock should be acquired");
        assert!(coordinator
            .acquire_lock("release:v1.2.3", Duration::from_millis(5_000))
            .expect("contended acquisition should succeed")
            .is_none());
        assert!(coordinator
            .renew_lock(
                "release:v1.2.3",
                &first.token,
                Duration::from_millis(5_000),
            )
            .expect("renew lock should succeed"));
        assert!(coordinator
            .release_lock("release:v1.2.3", &first.token)
            .expect("release lock should succeed"));
        assert!(coordinator
            .acquire_lock("release:v1.2.3", Duration::from_millis(5_000))
            .expect("lock should be available after release")
            .is_some());

        let expiring = coordinator
            .acquire_lock("build:42", Duration::from_millis(250))
            .expect("expiring lock should be acquired")
            .expect("expiring lock should exist");
        std::thread::sleep(Duration::from_millis(500));
        assert!(!coordinator
            .renew_lock("build:42", &expiring.token, Duration::from_millis(250))
            .expect("expired renew should return false"));
        assert!(coordinator
            .acquire_lock("build:42", Duration::from_millis(250))
            .expect("expired lock should be available")
            .is_some());

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn local_idempotency_claims_expire_and_can_be_forgotten() {
        let root = test_root("idempotency");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let coordinator = LocalCoordinator::new(&layout);
        assert!(coordinator
            .claim_idempotency("repo-1:v1.0.0", Duration::from_millis(5_000))
            .expect("first idempotency claim should succeed"));
        assert!(!coordinator
            .claim_idempotency("repo-1:v1.0.0", Duration::from_millis(5_000))
            .expect("duplicate idempotency claim should succeed"));
        coordinator
            .forget_idempotency("repo-1:v1.0.0")
            .expect("forget should succeed");
        assert!(coordinator
            .claim_idempotency("repo-1:v1.0.0", Duration::from_millis(5_000))
            .expect("claim should succeed after forget"));

        assert!(coordinator
            .claim_idempotency("repo-1:v2.0.0", Duration::from_millis(250))
            .expect("independent key should claim"));
        std::thread::sleep(Duration::from_millis(500));
        assert!(coordinator
            .claim_idempotency("repo-1:v2.0.0", Duration::from_millis(250))
            .expect("expired key should be claimable again"));

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn recover_runtime_state_requeues_running_work_and_releases_stale_leases() {
        let root = test_root("recovery");
        let directories = RuntimeDirectories::from_root(&root);
        let layout = StorageLayout::from_directories(&directories);
        initialize_database(&layout).expect("database bootstrap should succeed");

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let (build_run_id, publish_run_id) = seed_recovery_fixture(&connection);
        drop(connection);

        let report = recover_runtime_state(
            &layout,
            RECOVERY_INTERRUPTION_KIND_SYSTEM,
            "build attempt interrupted after an unexpected runtime interruption",
        )
        .expect("recovery should succeed");
        assert_eq!(
            report,
            RuntimeRecoveryReport {
                released_queue_message_leases: 1,
                cleared_coordination_leases: 1,
                requeued_build_runs: 1,
                requeued_publish_runs: 1,
                terminated_orphan_build_processes: 0,
                orphan_build_process_errors: 0,
                interrupted_builds: vec![InterruptedBuildRecoveryRecord {
                    build_run_id,
                    workspace_path: String::from("/tmp/workspace"),
                    log_path: Some(String::from("/tmp/build.log")),
                    interruption_kind: String::from(RECOVERY_INTERRUPTION_KIND_SYSTEM),
                    interruption_message: String::from(
                        "build attempt interrupted after an unexpected runtime interruption",
                    ),
                }],
            }
        );
        assert!(!report.is_empty());

        let connection = open_connection(&layout.database_path).expect("connection should open");
        let queue_lease_count: i64 = connection
            .query_row(
                "
                SELECT COUNT(1)
                FROM worker_queue_messages
                WHERE leased_by IS NOT NULL
                   OR lease_token IS NOT NULL
                   OR lease_expires_at_unix_millis IS NOT NULL
                ",
                [],
                |row| row.get(0),
            )
            .expect("queue lease count should load");
        assert_eq!(queue_lease_count, 0);

        let coordination_lease_count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_coordination_leases",
                [],
                |row| row.get(0),
            )
            .expect("coordination lease count should load");
        assert_eq!(coordination_lease_count, 0);

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
                           started_at,
                           current_stage_key,
                           current_stage_label,
                           current_stage_status
                    FROM build_runs
                    WHERE id = ?
                    ",
                    [build_run_id],
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
                .expect("requeued build run should load");
        assert_eq!(build_row.0, BuildStatus::Queued.as_str());
        assert!(build_row.1.is_none());
        assert!(build_row.2.is_none());
        assert!(build_row.3.is_none());
        assert!(build_row.4.is_none());
        assert!(build_row.5.is_none());
        assert!(build_row.6.is_none());
        assert!(build_row.7.is_none());

        let stage_row: (String, Option<String>) = connection
            .query_row(
                "
                SELECT status, error_message
                FROM build_run_steps
                WHERE build_run_id = ?
                  AND step_key = ?
                ",
                params![build_run_id, "checkout-repository"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("interrupted build stage should remain queryable");
        assert_eq!(stage_row.0, BuildStatus::Failed.as_str());
        assert_eq!(
            stage_row.1.as_deref(),
            Some("build attempt interrupted after an unexpected runtime interruption")
        );

        let publish_row: (String, Option<String>, Option<String>, Option<String>) = connection
            .query_row(
                "
                SELECT status, destination_ref, started_at, error_message
                FROM publish_runs
                WHERE id = ?
                ",
                [publish_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("requeued publish run should load");
        assert_eq!(publish_row.0, PublishStatus::Queued.as_str());
        assert!(publish_row.1.is_none());
        assert!(publish_row.2.is_none());
        assert!(publish_row.3.is_none());
        drop(connection);

        std::fs::remove_dir_all(root).expect("temporary database directory should be removable");
    }

    #[test]
    fn select_orphan_build_process_roots_matches_unity_process_by_workspace_path() {
        let interrupted_builds = vec![InterruptedBuildRecoveryRecord {
            build_run_id: 3,
            workspace_path: String::from(
                r"D:\Users\gabao\RevolutionsHandyGamesPublisherWorkspace\runs\build-run-3-attempt-25496-1778529831533390600",
            ),
            log_path: Some(String::from(
                r"D:\Users\gabao\RevolutionsHandyGamesPublisherWorkspace\runs\build-run-3-attempt-25496-1778529831533390600\logs\03-unity-build.log",
            )),
            interruption_kind: String::from(RECOVERY_INTERRUPTION_KIND_SYSTEM),
            interruption_message: String::from(
                "build attempt interrupted after an unexpected runtime interruption",
            ),
        }];
        let processes = vec![
            ObservedProcess {
                pid: 25496,
                parent_pid: Some(17664),
                name: String::from("hgp-runtime.exe"),
                command_line: String::from(
                    r#""C:\Users\gabao\projects\handy-games-publisher\target\debug\hgp-runtime.exe" serve"#,
                ),
            },
            ObservedProcess {
                pid: 4436,
                parent_pid: Some(25496),
                name: String::from("Unity.exe"),
                command_line: String::from(
                    r#""C:\Program Files\Unity\Hub\Editor\6000.4.3f1\Editor\Unity.exe" -batchmode -quit -nographics -logFile D:\Users\gabao\RevolutionsHandyGamesPublisherWorkspace\runs\build-run-3-attempt-25496-1778529831533390600\logs\03-unity-build.log -projectPath D:\Users\gabao\RevolutionsHandyGamesPublisherWorkspace\runs\build-run-3-attempt-25496-1778529831533390600\source"#,
                ),
            },
            ObservedProcess {
                pid: 9999,
                parent_pid: None,
                name: String::from("powershell.exe"),
                command_line: String::from(
                    r#"powershell -NoProfile -Command \"Get-Content -Tail 40 'D:\Users\gabao\RevolutionsHandyGamesPublisherWorkspace\runs\build-run-3-attempt-25496-1778529831533390600\logs\03-unity-build.log'\""#,
                ),
            },
        ];

        assert_eq!(
            select_orphan_build_process_roots(&interrupted_builds, &processes),
            vec![4436],
        );
    }

    fn table_exists(connection: &Connection, table_name: &str) -> bool {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'table' AND name = ?",
                [table_name],
                |row| row.get(0),
            )
            .expect("sqlite_master query should succeed");

        count == 1
    }

    fn table_has_columns(connection: &Connection, table_name: &str, expected: &[&str]) -> bool {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table_name})"))
            .expect("table_info query should prepare");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("table_info query should run");

        let mut column_names = Vec::new();
        for row in rows {
            column_names.push(row.expect("table_info row should scan"));
        }

        expected
            .iter()
            .all(|column_name| column_names.iter().any(|item| item == column_name))
    }

    fn index_exists(connection: &Connection, index_name: &str) -> bool {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE type = 'index' AND name = ?",
                [index_name],
                |row| row.get(0),
            )
            .expect("sqlite_master index query should succeed");

        count == 1
    }

    fn queue_message_exists(connection: &Connection, message_id: i64) -> bool {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_queue_messages WHERE id = ?",
                [message_id],
                |row| row.get(0),
            )
            .expect("queue message lookup should succeed");

        count == 1
    }

    fn queue_message_count(database_path: &std::path::Path, queue_name: &str) -> i64 {
        let connection = open_connection(database_path).expect("connection should open");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(1) FROM worker_queue_messages WHERE queue_name = ?",
                [queue_name],
                |row| row.get(0),
            )
            .expect("queue message count should succeed");
        drop(connection);

        count
    }

    fn build_run_count_for_release(connection: &Connection, release_run_id: i64) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM build_runs WHERE release_run_id = ?",
                [release_run_id],
                |row| row.get(0),
            )
            .expect("build run count should load")
    }

    fn publish_run_count_for_release(connection: &Connection, release_run_id: i64) -> i64 {
        connection
            .query_row(
                "SELECT COUNT(1) FROM publish_runs WHERE release_run_id = ?",
                [release_run_id],
                |row| row.get(0),
            )
            .expect("publish run count should load")
    }

    fn load_release_record(connection: &Connection, release_run_id: i64) -> ReleaseRunRecord {
        connection
            .query_row(
                "
                SELECT id,
                       repository_id,
                       git_tag,
                       git_commit,
                       trigger_source,
                       trigger_rule_id,
                       source_metadata_json,
                       source_identity,
                       engine_version,
                       status,
                       started_at,
                       finished_at,
                       error_message,
                       created_at,
                       updated_at
                FROM release_runs
                WHERE id = ?
                ",
                [release_run_id],
                super::scan_release_run_record,
            )
            .expect("release run should load")
    }

    fn assert_planned_build_runs(
        runs: &[BuildRunRecord],
        release_run_id: i64,
        primary_build_target_id: i64,
        secondary_build_target_id: i64,
    ) {
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].release_run_id, release_run_id);
        assert_eq!(runs[0].build_target_id, primary_build_target_id);
        assert_eq!(runs[0].status, BuildStatus::Queued.as_str());
        assert_eq!(runs[0].engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(
            runs[0].image_ref.as_deref(),
            Some(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
        );
        assert_eq!(runs[1].release_run_id, release_run_id);
        assert_eq!(runs[1].build_target_id, secondary_build_target_id);
        assert_eq!(runs[1].status, BuildStatus::Queued.as_str());
        assert_eq!(runs[1].engine_version.as_deref(), Some("2022.3.20f1"));
        assert_eq!(
            runs[1].image_ref.as_deref(),
            Some(DEFAULT_HOST_NATIVE_RUNNER_TYPE),
        );
    }

    fn enqueue_message(connection: &Connection, queue_name: &str, payload: &[u8]) -> i64 {
        connection
            .execute(
                "INSERT INTO worker_queue_messages (queue_name, payload) VALUES (?, ?)",
                params![queue_name, payload],
            )
            .expect("queue message should insert");

        connection.last_insert_rowid()
    }

    struct RepositoryFixture {
        repository_id: i64,
        primary_build_target_id: i64,
        secondary_build_target_id: i64,
        publish_target_id: i64,
    }

    struct BuildClaimFixture {
        build_run_id: i64,
    }

    struct BuildExecutionFixture {
        repository_id: i64,
        release_run_id: i64,
        build_target_id: i64,
        build_run_id: i64,
    }

    fn seed_repository_fixture(connection: &Connection, name: &str) -> RepositoryFixture {
        seed_repository_fixture_with_url(
            connection,
            name,
            &format!("https://example.com/{name}.git"),
        )
    }

    fn seed_repository_fixture_with_url(
        connection: &Connection,
        name: &str,
        repository_url: &str,
    ) -> RepositoryFixture {
        connection
            .execute(
                "INSERT INTO repositories (name, repo_url, engine_kind) VALUES (?, ?, ?)",
                params![name, repository_url, "unity"],
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
                    contract_json
                ) VALUES (?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    format!("{name}-windows"),
                    "player",
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                ],
            )
            .expect("primary build target should insert");
        let primary_build_target_id = connection.last_insert_rowid();

        connection
            .execute(
                "
                INSERT INTO build_targets (
                    repository_id,
                    name,
                    build_kind,
                    contract_json
                ) VALUES (?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    format!("{name}-linux"),
                    "player",
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneLinux64",
                            "buildMethod": "Builder.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                ],
            )
            .expect("secondary build target should insert");
        let secondary_build_target_id = connection.last_insert_rowid();

        connection
            .execute(
                "INSERT INTO publish_targets (repository_id, name, kind) VALUES (?, ?, ?)",
                params![repository_id, format!("{name}-publish"), "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();

        RepositoryFixture {
            repository_id,
            primary_build_target_id,
            secondary_build_target_id,
            publish_target_id,
        }
    }

    fn seed_build_execution_fixture(connection: &Connection, name: &str) -> BuildExecutionFixture {
        let repo = seed_repository_fixture(connection, name);
        connection
            .execute(
                "
                UPDATE repositories
                SET enabled = 1
                WHERE id = ?
                ",
                [repo.repository_id],
            )
            .expect("repository should remain enabled");
        connection
            .execute(
                "
                UPDATE build_targets
                SET build_kind = ?,
                    contract_json = ?,
                    output_kind = ?,
                    output_path_template = ?,
                    config_json = ?,
                    timeout_seconds = ?
                WHERE id = ?
                ",
                params![
                    "player",
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "CI.Build.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                    "archive",
                    "players/game.zip",
                    r#"{"optimize":true}"#,
                    900,
                    repo.primary_build_target_id,
                ],
            )
            .expect("build target execution metadata should update");

        let release_run_id = insert_release_run(
            connection,
            repo.repository_id,
            "v10.0.0",
            ReleaseStatus::Queued.as_str(),
        );
        connection
            .execute(
                "
                UPDATE release_runs
                SET git_commit = ?,
                    engine_version = ?
                WHERE id = ?
                ",
                params!["deadbeef", "2022.3.20f1", release_run_id],
            )
            .expect("release run execution metadata should update");

        let build_run_id = insert_build_run(
            connection,
            release_run_id,
            repo.primary_build_target_id,
            BuildStatus::Queued.as_str(),
        );
        update_build_run_plan(
            connection,
            build_run_id,
            "2022.3.20f1",
            DEFAULT_HOST_NATIVE_RUNNER_TYPE,
        );

        BuildExecutionFixture {
            repository_id: repo.repository_id,
            release_run_id,
            build_target_id: repo.primary_build_target_id,
            build_run_id,
        }
    }

    fn create_tagged_unity_repository(
        repository_path: &Path,
        git_tag: &str,
        unity_version: &str,
    ) -> String {
        if repository_path.exists() {
            std::fs::remove_dir_all(repository_path)
                .expect("existing repository fixture should be removable");
        }
        std::fs::create_dir_all(repository_path.join("ProjectSettings"))
            .expect("project settings directory should create");
        std::fs::write(
            repository_path.join(PROJECT_VERSION_FILE_PATH),
            format!("m_EditorVersion: {unity_version}\n"),
        )
        .expect("project version file should write");

        run_git_test_command(repository_path, &["init"]);
        run_git_test_command(
            repository_path,
            &["config", "user.name", "runtime-store-tests"],
        );
        run_git_test_command(
            repository_path,
            &["config", "user.email", "runtime-store-tests@example.com"],
        );
        run_git_test_command(repository_path, &["add", "."]);
        run_git_test_command(repository_path, &["commit", "-m", "seed unity version"]);
        run_git_test_command(repository_path, &["tag", git_tag]);

        let canonical_path = repository_path
            .canonicalize()
            .expect("repository fixture should canonicalize");
        if cfg!(windows) {
            let normalized_path = canonical_path.display().to_string();
            let normalized_path = normalized_path
                .strip_prefix(r"\\?\")
                .unwrap_or(&normalized_path)
                .replace('\\', "/");
            format!(
                "file:///{}",
                normalized_path
            )
        } else {
            canonical_path.display().to_string()
        }
    }

    struct FakeGitFixture {
        _env_lock: MutexGuard<'static, ()>,
        original_git_executable: Option<OsString>,
        original_path: Option<OsString>,
        log_path: PathBuf,
    }

    impl FakeGitFixture {
        fn install(root: &Path, required_token: &str, unity_version: &str) -> Self {
            let env_lock = test_environment_lock()
                .lock()
                .expect("test environment lock should acquire");
            let bin_dir = root.join("fake-git-bin");
            std::fs::create_dir_all(&bin_dir)
                .expect("fake git binary directory should create");
            let log_path = root.join("fake-git.log");
            let script_path = if cfg!(windows) {
                bin_dir.join("git.bat")
            } else {
                bin_dir.join("git")
            };

            std::fs::write(
                &script_path,
                fake_git_script(&log_path, required_token, unity_version),
            )
            .expect("fake git script should write");

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let mut permissions = std::fs::metadata(&script_path)
                    .expect("fake git script metadata should load")
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&script_path, permissions)
                    .expect("fake git script permissions should update");
            }

            let original_path = std::env::var_os("PATH");
            let original_git_executable =
                std::env::var_os("HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE");
            let mut path_value = OsString::from(bin_dir.as_os_str());
            path_value.push(if cfg!(windows) { ";" } else { ":" });
            if let Some(existing_path) = &original_path {
                path_value.push(existing_path);
            }
            std::env::set_var("PATH", &path_value);
            std::env::set_var(
                "HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE",
                &script_path,
            );

            Self {
                _env_lock: env_lock,
                original_git_executable,
                original_path,
                log_path,
            }
        }

        fn log_path(&self) -> &Path {
            &self.log_path
        }
    }

    impl Drop for FakeGitFixture {
        fn drop(&mut self) {
            if let Some(path) = &self.original_git_executable {
                std::env::set_var("HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE", path);
            } else {
                std::env::remove_var("HANDY_GAMES_PUBLISHER_TEST_GIT_EXECUTABLE");
            }

            if let Some(path) = &self.original_path {
                std::env::set_var("PATH", path);
            } else {
                std::env::remove_var("PATH");
            }
        }
    }

    fn test_environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn fake_git_script(log_path: &Path, required_token: &str, unity_version: &str) -> String {
        let log_path = log_path.display();
        if cfg!(windows) {
            return format!(
                concat!(
                    "@echo off\r\n",
                    "setlocal EnableExtensions EnableDelayedExpansion\r\n",
                    "set \"args=%*\"\r\n",
                    "echo %args%>>\"{0}\"\r\n",
                    "set \"saw_remote_command=0\"\r\n",
                    "set \"last_command=\"\r\n",
                    "set \"last=\"\r\n",
                    "for %%A in (%*) do (\r\n",
                    "  if /I \"%%~A\"==\"clone\" (set \"saw_remote_command=1\" & set \"last_command=clone\")\r\n",
                    "  if /I \"%%~A\"==\"checkout\" (set \"saw_remote_command=1\" & set \"last_command=checkout\")\r\n",
                    "  set \"last=%%~A\"\r\n",
                    ")\r\n",
                    "if \"!saw_remote_command!\"==\"1\" (\r\n",
                    "  echo !args! | findstr /C:\"{1}\" >nul || exit /b 23\r\n",
                    ")\r\n",
                    "if \"!last_command!\"==\"clone\" (\r\n",
                    "  mkdir \"!last!\\ProjectSettings\" 2>nul\r\n",
                    "  > \"!last!\\ProjectSettings\\ProjectVersion.txt\" ",
                    "echo m_EditorVersion: {2}\r\n",
                    ")\r\n",
                    "exit /b 0\r\n"
                ),
                log_path,
                required_token,
                unity_version,
            );
        }

        format!(
            concat!(
                "#!/bin/sh\n",
                "set -eu\n",
                "args=\"$*\"\n",
                "printf '%s\\n' \"$args\" >> '{0}'\n",
                "saw_remote_command=0\n",
                "last_command=''\n",
                "last=''\n",
                "for arg in \"$@\"; do\n",
                "  if [ \"$arg\" = 'clone' ]; then\n",
                "    saw_remote_command=1\n",
                "    last_command='clone'\n",
                "  fi\n",
                "  if [ \"$arg\" = 'checkout' ]; then\n",
                "    saw_remote_command=1\n",
                "    last_command='checkout'\n",
                "  fi\n",
                "  last=\"$arg\"\n",
                "done\n",
                "if [ \"$saw_remote_command\" = '1' ]; then\n",
                "  case \"$args\" in\n",
                "    *'{1}'*) ;;\n",
                "    *) exit 23 ;;\n",
                "  esac\n",
                "fi\n",
                "if [ \"$last_command\" = 'clone' ]; then\n",
                "  mkdir -p \"$last/ProjectSettings\"\n",
                "  printf 'm_EditorVersion: {2}\\n' > ",
                "\"$last/ProjectSettings/ProjectVersion.txt\"\n",
                "fi\n",
                "exit 0\n"
            ),
            log_path,
            required_token,
            unity_version,
        )
    }

    fn insert_git_bearer_credentials(
        connection: &Connection,
        name: &str,
        token: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO credentials (name, kind, config_json) VALUES (?, ?, ?)",
                params![
                    name,
                    "git-http-bearer",
                    format!(r#"{{"token":"{token}"}}"#),
                ],
            )
            .expect("credential should insert");

        connection.last_insert_rowid()
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

    fn insert_release_run(
        connection: &Connection,
        repository_id: i64,
        git_tag: &str,
        status: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO release_runs (repository_id, git_tag, status) VALUES (?, ?, ?)",
                params![repository_id, git_tag, status],
            )
            .expect("release run should insert");

        connection.last_insert_rowid()
    }

    fn update_release_run_engine_version(
        connection: &Connection,
        release_run_id: i64,
        engine_version: &str,
    ) {
        connection
            .execute(
                "UPDATE release_runs SET engine_version = ? WHERE id = ?",
                params![engine_version, release_run_id],
            )
            .expect("release unity version should update");
    }

    fn seed_manual_release_for_rebuild(
        connection: &Connection,
        repository_id: i64,
        git_tag: &str,
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
                    engine_version,
                    status
                ) VALUES (?, ?, ?, ?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    git_tag,
                    "deadbeef",
                    TRIGGER_SOURCE_MANUAL,
                    r#"{"requested_via":"cli"}"#,
                    "2022.3.20f1",
                    ReleaseStatus::Queued.as_str(),
                ],
            )
            .expect("seed manual release should insert");

        connection.last_insert_rowid()
    }

    fn insert_build_run(
        connection: &Connection,
        release_run_id: i64,
        build_target_id: i64,
        status: &str,
    ) -> i64 {
        connection
            .execute(
                "INSERT INTO build_runs (release_run_id, build_target_id, status) VALUES (?, ?, ?)",
                params![release_run_id, build_target_id, status],
            )
            .expect("build run should insert");

        connection.last_insert_rowid()
    }

    fn update_build_run_plan(
        connection: &Connection,
        build_run_id: i64,
        engine_version: &str,
        image_ref: &str,
    ) {
        connection
            .execute(
                "
                UPDATE build_runs
                SET engine_version = ?, image_ref = ?
                WHERE id = ?
                ",
                params![engine_version, image_ref, build_run_id],
            )
            .expect("build run plan should update");
    }

    fn insert_artifact(connection: &Connection, build_run_id: i64, name: &str) -> i64 {
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
                ) VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    build_run_id,
                    name,
                    "archive",
                    format!("artifacts/{name}"),
                    "runtime_artifact",
                    format!("artifacts/{name}"),
                ],
            )
            .expect("artifact should insert");

        connection.last_insert_rowid()
    }

    fn insert_build_publish_binding(
        connection: &Connection,
        build_target_id: i64,
        publish_target_id: i64,
        enabled: bool,
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
                params![
                    build_target_id,
                    publish_target_id,
                    if enabled { 1 } else { 0 },
                    "{}",
                ],
            )
            .expect("build publish binding should insert");

        connection.last_insert_rowid()
    }

    fn insert_publish_run(
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
                    status,
                    execution_contract_json
                ) VALUES (?, ?, ?, ?, ?, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    status,
                    "{}",
                ],
            )
            .expect("publish run should insert");

        connection.last_insert_rowid()
    }

    fn seed_build_claim_fixture(
        connection: &Connection,
        name: &str,
        git_tag: &str,
        build_status: &str,
    ) -> BuildClaimFixture {
        let repo = seed_repository_fixture(connection, name);
        let release_run_id = insert_release_run(connection, repo.repository_id, git_tag, "queued");
        let build_run_id = insert_build_run(
            connection,
            release_run_id,
            repo.primary_build_target_id,
            build_status,
        );

        BuildClaimFixture { build_run_id }
    }

    fn seed_recovery_fixture(connection: &Connection) -> (i64, i64) {
        connection
            .execute(
                "
                INSERT INTO repositories (name, repo_url, engine_kind)
                VALUES (?, ?, ?)
                ",
                params!["repo", "https://example.com/repo.git", "unity"],
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
                    contract_json
                )
                VALUES (?, ?, ?, ?)
                ",
                params![
                    repository_id,
                    "windows-player",
                    "player",
                    serde_json::json!({
                        "unity": {
                            "targetPlatform": "StandaloneWindows64",
                            "buildMethod": "Builder.Perform",
                            "editorVersion": ""
                        }
                    })
                    .to_string(),
                ],
            )
            .expect("build target should insert");
        let build_target_id = connection.last_insert_rowid();

        connection
            .execute(
                "
                INSERT INTO publish_targets (repository_id, name, kind)
                VALUES (?, ?, ?)
                ",
                params![repository_id, "filesystem", "filesystem"],
            )
            .expect("publish target should insert");
        let publish_target_id = connection.last_insert_rowid();

        connection
            .execute(
                "
                INSERT INTO release_runs (repository_id, git_tag, status)
                VALUES (?, ?, ?)
                ",
                params![repository_id, "v1.0.0", "queued"],
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
                    log_path,
                    artifact_root_path,
                    current_stage_key,
                    current_stage_label,
                    current_stage_status,
                    heartbeat_at,
                    last_progress_message,
                    started_at,
                    error_message
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?, CURRENT_TIMESTAMP, ?)
                ",
                params![
                    release_run_id,
                    build_target_id,
                    BuildStatus::Running.as_str(),
                    "/tmp/workspace",
                    "/tmp/build.log",
                    "/tmp/artifacts",
                    "checkout-repository",
                    "Checkout Repository",
                    BuildStatus::Running.as_str(),
                    "cloning repository",
                    "interrupted mid-build",
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
                    last_message,
                    heartbeat_at,
                    started_at,
                    updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                ",
                params![
                    build_run_id,
                    2,
                    "checkout-repository",
                    "Checkout Repository",
                    BuildStatus::Running.as_str(),
                    "/tmp/workspace/logs/02-checkout-repository.log",
                    "cloning repository",
                ],
            )
            .expect("running build stage should insert");

        connection
            .execute(
                "
                INSERT INTO artifacts (build_run_id, name, kind, path)
                VALUES (?, ?, ?, ?)
                ",
                params![build_run_id, "game.zip", "archive", "artifacts/game.zip"],
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
                    destination_ref,
                    started_at,
                    error_message
                ) VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?)
                ",
                params![
                    release_run_id,
                    build_run_id,
                    publish_target_id,
                    artifact_id,
                    PublishStatus::Running.as_str(),
                    "out/releases/v1.0.0",
                    "interrupted mid-publish",
                ],
            )
            .expect("publish run should insert");
        let publish_run_id = connection.last_insert_rowid();

        connection
            .execute(
                "
                INSERT INTO worker_queue_messages (
                    queue_name,
                    payload,
                    leased_by,
                    lease_token,
                    lease_expires_at_unix_millis,
                    dequeue_count
                ) VALUES (?, ?, ?, ?, ?, ?)
                ",
                params!["builds", br#"{"build_run_id":1}"#, "worker-a", "token-a", 9_999_999_999_i64, 1],
            )
            .expect("queue message should insert");
        connection
            .execute(
                "
                INSERT INTO worker_coordination_leases (
                    name,
                    token,
                    lease_expires_at_unix_millis
                ) VALUES (?, ?, ?)
                ",
                params!["release-plan:1", "lock-a", 9_999_999_999_i64],
            )
            .expect("coordination lease should insert");

        (build_run_id, publish_run_id)
    }

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "handy-games-publisher-runtime-store-{label}-{}",
            std::process::id()
        ))
    }
    }