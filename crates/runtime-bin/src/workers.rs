//! Hosts the runtime polling, supervision, and worker loop orchestration that
//! drives the local automation host between individual command handlers.

use super::*;

use std::time::Instant;

const POLL_FAILURE_STAGE_ADVANCE_RELEASE_QUEUE_PRECHECK: &str =
    "advance_release_queue_precheck";
const POLL_FAILURE_STAGE_POLL_REMOTE: &str = "poll_remote";
const POLL_FAILURE_STAGE_ADVANCE_RELEASE_QUEUE_POST_POLL: &str =
    "advance_release_queue_post_poll";

#[derive(Debug, Serialize)]
struct FailedPollAttemptLogRecord<'a> {
    attempted_at_unix_millis: u128,
    repository_id: i64,
    repository_name: &'a str,
    repository_url: &'a str,
    polling_interval_seconds: i64,
    last_seen_tag: Option<&'a str>,
    stage: &'a str,
    error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeLoopCadence {
    worker_loop_interval: Duration,
    heartbeat_interval: Duration,
}

impl RuntimeLoopCadence {
    pub(crate) fn from_config(config: &RuntimeConfig) -> Self {
        Self {
            worker_loop_interval: Duration::from_millis(
                config.runtime_loop.worker_loop_interval_millis,
            ),
            heartbeat_interval: Duration::from_millis(
                config.runtime_loop.heartbeat_interval_millis,
            ),
        }
    }

    pub(crate) fn worker_loop_interval(self) -> Duration {
        self.worker_loop_interval
    }

    pub(crate) fn should_emit_heartbeat(self, elapsed_since_last_heartbeat: Duration) -> bool {
        elapsed_since_last_heartbeat >= self.heartbeat_interval
    }
}

pub(crate) fn run_release_planner_cycle(
    storage: &StorageLayout,
) -> Result<bool, Box<dyn Error>> {
    let coordinator = LocalCoordinator::new(storage);

    coordinator
        .process_next_release_job(
            RELEASE_PLANNER_WORKER_NAME,
            Duration::ZERO,
            RELEASE_QUEUE_LEASE_TTL,
        )
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn run_build_worker_cycle(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<bool, Box<dyn Error>> {
    Ok(run_build_run_next_command(&[], config, storage)? != "null")
}

fn run_publish_worker_cycle(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<bool, Box<dyn Error>> {
    Ok(run_publish_run_next_command(&[], config, storage)? != "null")
}

pub(crate) fn run_runtime_worker_iteration(
    config: &RuntimeConfig,
    storage: &StorageLayout,
    poll_schedule: &mut RepositoryPollSchedule,
) -> Result<(), Box<dyn Error>> {
    let coordinator = LocalCoordinator::new(storage);
    let forced_repository_ids = match take_forced_repository_poll_ids(storage) {
        Ok(repository_ids) => repository_ids,
        Err(error) => {
            eprintln!(
                "failed to consume runtime control requests before polling: {error}"
            );
            HashSet::new()
        }
    };
    let _ = run_repository_poll_cycle_with_forced_repositories(
        &coordinator,
        storage,
        Some(poll_schedule),
        &forced_repository_ids,
    )?;
    // RetryLater planner messages must not monopolize the serve loop, or
    // queued build work for earlier releases never gets a chance to start.
    let _ = run_release_planner_cycle(storage)?;
    while run_build_worker_cycle(config, storage)? {}
    while run_publish_worker_cycle(config, storage)? {}

    Ok(())
}

/// Tracks the next eligible poll instant for each repository between worker cycles.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct RepositoryPollSchedule {
    pub(crate) next_poll_at_by_repository: HashMap<i64, SystemTime>,
}

impl RepositoryPollSchedule {
    fn is_due(&self, repository_id: i64, now: SystemTime) -> bool {
        self.next_poll_at_by_repository
            .get(&repository_id)
            .is_none_or(|next_poll_at| *next_poll_at <= now)
    }

    fn set_next_poll_at(&mut self, repository_id: i64, now: SystemTime, interval: Duration) {
        self.next_poll_at_by_repository
            .insert(repository_id, now + interval);
    }

    fn delete_repository(&mut self, repository_id: i64) {
        self.next_poll_at_by_repository.remove(&repository_id);
    }

    fn retain_repositories(&mut self, repositories: &HashSet<i64>) {
        self.next_poll_at_by_repository
            .retain(|repository_id, _| repositories.contains(repository_id));
    }
}

fn take_forced_repository_poll_ids(
    storage: &StorageLayout,
) -> io::Result<HashSet<i64>> {
    let requests = take_runtime_control_requests(storage)?;
    let mut repository_ids = HashSet::new();

    for request in requests {
        match request {
            RuntimeControlRequest::ForceRepositoryPoll { repository_id } => {
                repository_ids.insert(repository_id);
            }
        }
    }

    Ok(repository_ids)
}

pub(crate) fn run_repository_poll_cycle(
    coordinator: &LocalCoordinator,
    storage: &StorageLayout,
    poll_schedule: Option<&mut RepositoryPollSchedule>,
) -> io::Result<AutomationPollReport> {
    run_repository_poll_cycle_with_forced_repositories(
        coordinator,
        storage,
        poll_schedule,
        &HashSet::new(),
    )
}

fn run_repository_poll_cycle_with_forced_repositories(
    coordinator: &LocalCoordinator,
    storage: &StorageLayout,
    mut poll_schedule: Option<&mut RepositoryPollSchedule>,
    forced_repository_ids: &HashSet<i64>,
) -> io::Result<AutomationPollReport> {
    let repositories = coordinator.list_polling_repositories()?;
    let tag_lister = GitTagLister::default();
    let now = SystemTime::now();
    let mut seen_repositories = HashSet::with_capacity(repositories.len());
    let mut results = Vec::new();

    for repository in repositories {
        seen_repositories.insert(repository.id);
        let force_poll = forced_repository_ids.contains(&repository.id);

        if repository.auth_binding_status == REPOSITORY_AUTH_BINDING_STATUS_REQUIRED_UNBOUND
            && !force_poll
        {
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_REQUIRED_UNBOUND,
            ));
            continue;
        }

        if repository.auth_binding_status == REPOSITORY_AUTH_BINDING_STATUS_REAUTH_REQUIRED
            && !force_poll
        {
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_REAUTH_REQUIRED,
            ));
            continue;
        }

        if !repository.enabled {
            if let Some(schedule) = poll_schedule.as_deref_mut() {
                schedule.delete_repository(repository.id);
            }
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_DISABLED,
            ));
            continue;
        }

        if repository.enabled_build_target_count == 0 {
            if let Some(schedule) = poll_schedule.as_deref_mut() {
                schedule.delete_repository(repository.id);
            }
            results.push(skipped_poll_result(
                &repository,
                POLL_STATUS_SKIPPED_NO_ENABLED_BUILD_TARGETS,
            ));
            continue;
        }

        if !force_poll {
            match coordinator.advance_repository_release_queue(repository.id) {
                Ok(true) => {
                    results.push(skipped_poll_result(
                        &repository,
                        POLL_STATUS_SKIPPED_ACTIVE_RELEASE_BACKLOG,
                    ));
                    continue;
                }
                Ok(false) => {}
                Err(error) => {
                    log_failed_poll_attempt(
                        storage,
                        &repository,
                        POLL_FAILURE_STAGE_ADVANCE_RELEASE_QUEUE_PRECHECK,
                        &error,
                    );
                    results.push(error_poll_result(&repository, error));
                    continue;
                }
            }
        }

        if let Some(schedule) = poll_schedule.as_deref_mut() {
            if !force_poll && !schedule.is_due(repository.id, now) {
                continue;
            }
            schedule.set_next_poll_at(
                repository.id,
                now,
                Duration::from_secs(repository.polling_interval_seconds as u64),
            );
        }

        match poll_repository(coordinator, storage, &tag_lister, &repository) {
            Ok(result) => {
                if !result.queued_release_ids.is_empty() {
                    if let Err(error) = coordinator.advance_repository_release_queue(repository.id)
                    {
                        log_failed_poll_attempt(
                            storage,
                            &repository,
                            POLL_FAILURE_STAGE_ADVANCE_RELEASE_QUEUE_POST_POLL,
                            &error,
                        );
                        results.push(error_poll_result(&repository, error));
                        continue;
                    }
                }
                results.push(result);
            }
            Err(error) => {
                log_failed_poll_attempt(
                    storage,
                    &repository,
                    POLL_FAILURE_STAGE_POLL_REMOTE,
                    &error,
                );
                if is_authentication_poll_error(&error) {
                    persist_repository_auth_runtime_failure(coordinator, repository.id, &error);
                    log_poll_auth_failure_event(
                        storage,
                        &repository,
                        POLL_FAILURE_STAGE_POLL_REMOTE,
                        &error,
                    );
                    results.push(error_poll_result(&repository, error));
                    continue;
                }
                results.push(error_poll_result(&repository, error));
            }
        }
    }

    if let Some(schedule) = poll_schedule {
        schedule.retain_repositories(&seen_repositories);
    }

    Ok(AutomationPollReport { repositories: results })
}

/// Polls one repository by listing remote tags and queuing unseen tags only.
/// Branch heads are not inspected by the polling loop.
fn poll_repository(
    coordinator: &LocalCoordinator,
    storage: &StorageLayout,
    tag_lister: &GitTagLister,
    repository: &PollingRepositoryRecord,
) -> io::Result<RepositoryPollResult> {
    let git_auth = resolve_repository_git_auth(coordinator, repository.credentials_id)?;
    let tags = tag_lister.list_tags(&GitTagListRequest {
        repository_url: repository.repo_url.clone(),
        auth: git_auth,
    })?;
    if !repository.has_release_history
        && repository
            .last_seen_tag
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
    {
        return baseline_latest_repository_tag_without_process_history(
            coordinator,
            repository,
            &tags,
        );
    }

    let (selected_tags, status, ok) =
        select_queued_repository_tags(&tags, repository.last_seen_tag.as_deref());
    if !ok {
        return Ok(RepositoryPollResult {
            repository_id: repository.id,
            repository_name: repository.name.clone(),
            status: status.to_owned(),
            error: None,
            last_seen_tag_before: repository.last_seen_tag.clone(),
            last_seen_tag_after: repository.last_seen_tag.clone(),
            discovered_tags: Vec::new(),
            queued_release_ids: Vec::new(),
        });
    }

    let mut queued_release_ids = Vec::new();
    let mut discovered_tags = Vec::new();
    let mut last_seen_tag_after = repository.last_seen_tag.clone();

    for tag in selected_tags {
        match coordinator.dispatch_repository_poll_release(RepositoryPollDispatchInput {
            repository_id: repository.id,
            git_tag: tag.name.clone(),
            git_commit: tag.commit.clone(),
            observed_via: POLL_OBSERVED_VIA.to_owned(),
        }) {
            Ok(release) => {
                coordinator.update_repository_last_seen_tag(repository.id, &tag.name)?;
                last_seen_tag_after = Some(tag.name.clone());
                let context = ReleaseEventContext {
                    release_run_id: release.id,
                    repository_id: release.repository_id,
                    repository_name: repository.name.clone(),
                    git_tag: release.git_tag.clone(),
                    git_commit: release.git_commit.clone(),
                    user_requested: user_requested_from_trigger_source(&release.trigger_source),
                };
                if let Err(error) = emit_tag_detected_event(storage, &context) {
                    log_runtime_event_failure(EVENT_TOPIC_TAG_DETECTED, &error);
                }
                if let Err(error) = emit_release_queued_event(storage, &context) {
                    log_runtime_event_failure(EVENT_TOPIC_RELEASE_QUEUED, &error);
                }
                discovered_tags.push(tag);
                queued_release_ids.push(release.id);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                coordinator.update_repository_last_seen_tag(repository.id, &tag.name)?;
                last_seen_tag_after = Some(tag.name.clone());
                discovered_tags.push(tag);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                let status = if queued_release_ids.is_empty() {
                    POLL_STATUS_BUILD_IN_PROGRESS
                } else {
                    POLL_STATUS_QUEUED
                };
                return Ok(RepositoryPollResult {
                    repository_id: repository.id,
                    repository_name: repository.name.clone(),
                    status: status.to_owned(),
                    error: None,
                    last_seen_tag_before: repository.last_seen_tag.clone(),
                    last_seen_tag_after,
                    discovered_tags,
                    queued_release_ids,
                });
            }
            Err(error) => return Err(error),
        }
    }

    let status = if !queued_release_ids.is_empty() {
        POLL_STATUS_QUEUED
    } else {
        POLL_STATUS_ALREADY_SEEN
    };

    Ok(RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: status.to_owned(),
        error: None,
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after,
        discovered_tags,
        queued_release_ids,
    })
}

fn baseline_latest_repository_tag_without_process_history(
    coordinator: &LocalCoordinator,
    repository: &PollingRepositoryRecord,
    tags: &[GitTag],
) -> io::Result<RepositoryPollResult> {
    let Some(latest_tag) = tags.last().cloned() else {
        return Ok(RepositoryPollResult {
            repository_id: repository.id,
            repository_name: repository.name.clone(),
            status: POLL_STATUS_NO_TAGS.to_owned(),
            error: None,
            last_seen_tag_before: repository.last_seen_tag.clone(),
            last_seen_tag_after: repository.last_seen_tag.clone(),
            discovered_tags: Vec::new(),
            queued_release_ids: Vec::new(),
        });
    };

    let normalized_last_seen = repository.last_seen_tag.as_deref().unwrap_or_default().trim();
    if normalized_last_seen == latest_tag.name {
        return Ok(RepositoryPollResult {
            repository_id: repository.id,
            repository_name: repository.name.clone(),
            status: POLL_STATUS_UNCHANGED.to_owned(),
            error: None,
            last_seen_tag_before: repository.last_seen_tag.clone(),
            last_seen_tag_after: repository.last_seen_tag.clone(),
            discovered_tags: Vec::new(),
            queued_release_ids: Vec::new(),
        });
    }

    coordinator.update_repository_last_seen_tag(repository.id, &latest_tag.name)?;

    Ok(RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: POLL_STATUS_ALREADY_SEEN.to_owned(),
        error: None,
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after: Some(latest_tag.name.clone()),
        discovered_tags: vec![latest_tag],
        queued_release_ids: Vec::new(),
    })
}

pub(crate) fn resolve_repository_git_auth(
    coordinator: &LocalCoordinator,
    credentials_id: Option<i64>,
) -> io::Result<GitAuthOptions> {
    let Some(credentials_id) = credentials_id else {
        return Ok(GitAuthOptions::default());
    };

    let credentials = coordinator.get_credential_record(credentials_id)?;
    let resolved_config_json = runtime_store::resolve_credential_secret_config_json(
        &credentials.kind,
        &credentials.config_json,
    )?;

    git_auth_options_from_credentials(&credentials.kind, &resolved_config_json)
}

pub(crate) fn failed_poll_attempt_log_path(
    storage: &StorageLayout,
    repository: &PollingRepositoryRecord,
) -> PathBuf {
    let logs_root = storage
        .runtime_log_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    logs_root
        .join("repositories")
        .join(format!(
            "repository-{}-{}",
            repository.id,
            repository_log_slug(&repository.name)
        ))
        .join("failed-poll-attempts.jsonl")
}

pub(crate) fn record_failed_poll_attempt(
    storage: &StorageLayout,
    repository: &PollingRepositoryRecord,
    stage: &str,
    error: &io::Error,
) -> io::Result<PathBuf> {
    let log_path = failed_poll_attempt_log_path(storage, repository);
    let parent = log_path.parent().ok_or_else(|| {
        io::Error::other("failed poll attempt log path is missing a parent directory")
    })?;
    fs::create_dir_all(parent)?;

    let record = FailedPollAttemptLogRecord {
        attempted_at_unix_millis: SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        repository_id: repository.id,
        repository_name: &repository.name,
        repository_url: &repository.repo_url,
        polling_interval_seconds: repository.polling_interval_seconds,
        last_seen_tag: repository.last_seen_tag.as_deref(),
        stage,
        error: error.to_string(),
    };

    let encoded = serde_json::to_string(&record)
        .map_err(|serialization_error| io::Error::new(ErrorKind::InvalidData, serialization_error))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    writeln!(file, "{encoded}")?;

    Ok(log_path)
}

fn log_failed_poll_attempt(
    storage: &StorageLayout,
    repository: &PollingRepositoryRecord,
    stage: &str,
    error: &io::Error,
) {
    if let Err(log_error) = record_failed_poll_attempt(storage, repository, stage, error) {
        eprintln!(
            "failed to persist poll failure log for repository {}: {}",
            repository.id,
            log_error
        );
    }
}

fn is_authentication_poll_error(error: &io::Error) -> bool {
    error_indicates_authentication_failure(error)
}

fn log_poll_auth_failure_event(
    storage: &StorageLayout,
    repository: &PollingRepositoryRecord,
    stage: &str,
    error: &io::Error,
) {
    let summary = format!(
        "Automatic polling paused for {} after an authentication failure",
        repository.name
    );

    if let Err(event_error) = emit_runtime_event(
        storage,
        RuntimeEventInput {
            topic: String::from(EVENT_TOPIC_POLL_AUTH_FAILED),
            severity: String::from("error"),
            origin: String::from("runtime-bin"),
            user_requested: false,
            repository_id: Some(repository.id),
            release_run_id: None,
            build_run_id: None,
            publish_run_id: None,
            summary,
            payload: serde_json::json!({
                "repository_name": &repository.name,
                "repository_url": &repository.repo_url,
                "polling_interval_seconds": repository.polling_interval_seconds,
                "last_seen_tag": &repository.last_seen_tag,
                "stage": stage,
                "error": error.to_string(),
                "worker_action": "mark_reauth_required",
            }),
        },
    ) {
        eprintln!(
            "failed to emit poll auth failure event for repository {}: {}",
            repository.id,
            event_error,
        );
    }
}

fn repository_log_slug(name: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_separator = false;

    for character in name.chars() {
        let lowered = character.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            slug.push(lowered);
            previous_was_separator = false;
            continue;
        }

        if !previous_was_separator && !slug.is_empty() {
            slug.push('-');
            previous_was_separator = true;
        }
    }

    if slug.is_empty() {
        String::from("repository")
    } else {
        slug.trim_matches('-').to_owned()
    }
}

pub(crate) fn resolve_registration_checkout_ref(
    repository: &RepositoryCheckoutRecord,
    repository_url: &str,
    git_auth: &GitAuthOptions,
    explicit_git_ref: Option<String>,
) -> io::Result<(String, String)> {
    if let Some(git_ref) = explicit_git_ref {
        return Ok((git_ref, String::from("explicit")));
    }

    if let Some(default_branch) = repository.default_branch.clone() {
        return Ok((default_branch, String::from("default_branch")));
    }

    let remote_head = GitRemoteHeadRefResolver::new()
        .resolve_head_ref(&GitRemoteHeadRefRequest {
            repository_url: repository_url.to_owned(),
            auth: git_auth.clone(),
        })
        .map_err(|error| {
            io::Error::other(format!(
                "repository {} is missing default_branch and remote HEAD could not be resolved: {error}",
                repository.id
            ))
        })?;

    Ok((remote_head, String::from("remote_head")))
}

pub(crate) fn resolve_registration_checkout_workspace_root(
    config: &RuntimeConfig,
    repository: &RepositoryCheckoutRecord,
) -> PathBuf {
    repository
        .workspace_root_override
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            config
                .directories
                .data_dir
                .join("repositories")
                .join(format!("repository-{}", repository.id))
        })
}

pub(crate) fn read_checked_out_head_commit(source_path: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(source_path)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "read checked out HEAD from {:?}: exit code {:?}; stderr: {}",
            source_path,
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }

    let head_commit = String::from_utf8_lossy(&output.stdout);
    let trimmed = head_commit.trim();
    if trimmed.is_empty() {
        return Err(io::Error::other(format!(
            "read checked out HEAD from {:?}: git returned an empty commit id",
            source_path,
        )));
    }

    Ok(trimmed.to_owned())
}

pub(crate) fn select_queued_repository_tags(
    tags: &[GitTag],
    last_seen_tag: Option<&str>,
) -> (Vec<GitTag>, &'static str, bool) {
    if tags.is_empty() {
        return (Vec::new(), POLL_STATUS_NO_TAGS, false);
    }

    let normalized_last_seen = last_seen_tag.unwrap_or_default().trim();
    if normalized_last_seen.is_empty() {
        return (tags.to_vec(), "", true);
    }

    for (index, tag) in tags.iter().enumerate() {
        if tag.name != normalized_last_seen {
            continue;
        }

        if index == tags.len() - 1 {
            return (Vec::new(), POLL_STATUS_UNCHANGED, false);
        }

        return (tags[index + 1..].to_vec(), "", true);
    }

    if tags.last().is_some_and(|tag| tag.name == normalized_last_seen) {
        return (Vec::new(), POLL_STATUS_UNCHANGED, false);
    }

    (vec![tags[tags.len() - 1].clone()], "", true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_tag(name: &str, commit: &str) -> GitTag {
        GitTag {
            name: name.to_owned(),
            commit: commit.to_owned(),
        }
    }

    #[test]
    fn select_queued_repository_tags_returns_all_tags_when_no_tag_was_seen_yet() {
        let tags = vec![git_tag("v1.0.0", "a1"), git_tag("v1.1.0", "b2")];

        let (selected, status, ok) = select_queued_repository_tags(&tags, None);

        assert!(ok);
        assert_eq!(status, "");
        assert_eq!(selected, tags);
    }

    #[test]
    fn select_queued_repository_tags_advances_only_from_last_seen_tag() {
        let tags = vec![
            git_tag("v1.0.0", "a1"),
            git_tag("v1.1.0", "b2"),
            git_tag("v1.2.0", "c3"),
        ];

        let (selected, status, ok) =
            select_queued_repository_tags(&tags, Some("v1.0.0"));

        assert!(ok);
        assert_eq!(status, "");
        assert_eq!(selected, vec![git_tag("v1.1.0", "b2"), git_tag("v1.2.0", "c3")]);
    }

    #[test]
    fn select_queued_repository_tags_reports_unchanged_when_latest_tag_was_seen() {
        let tags = vec![git_tag("v1.0.0", "a1"), git_tag("v1.1.0", "b2")];

        let (selected, status, ok) =
            select_queued_repository_tags(&tags, Some("v1.1.0"));

        assert!(!ok);
        assert_eq!(status, POLL_STATUS_UNCHANGED);
        assert!(selected.is_empty());
    }
}

fn skipped_poll_result(
    repository: &PollingRepositoryRecord,
    status: &str,
) -> RepositoryPollResult {
    RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: status.to_owned(),
        error: None,
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after: repository.last_seen_tag.clone(),
        discovered_tags: Vec::new(),
        queued_release_ids: Vec::new(),
    }
}

fn error_poll_result(
    repository: &PollingRepositoryRecord,
    error: io::Error,
) -> RepositoryPollResult {
    RepositoryPollResult {
        repository_id: repository.id,
        repository_name: repository.name.clone(),
        status: POLL_STATUS_ERROR.to_owned(),
        error: Some(error.to_string()),
        last_seen_tag_before: repository.last_seen_tag.clone(),
        last_seen_tag_after: repository.last_seen_tag.clone(),
        discovered_tags: Vec::new(),
        queued_release_ids: Vec::new(),
    }
}

pub(crate) fn serve_runtime(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<(), Box<dyn Error>> {
    let executable = env::current_exe()?;
    let attempt = current_supervision_attempt();
    let snapshot = bootstrap_runtime(
        config,
        storage,
        &executable,
        RuntimeRestartPolicy::from_settings(&config.supervision),
    )?;
    let coordinator = LocalCoordinator::new(storage);
    recover_interrupted_build_attempts(&coordinator, &snapshot.recovery_report);
    let mut report = snapshot.health_report;
    let mut heartbeat_count = 0_u32;
    let mut poll_schedule = RepositoryPollSchedule::default();
    let cadence = RuntimeLoopCadence::from_config(config);
    let mut last_heartbeat_at = Instant::now();

    loop {
        if runtime_stop_requested(storage)? {
            return Ok(());
        }

        if let Err(error) = run_runtime_worker_iteration(config, storage, &mut poll_schedule) {
            if let Err(health_error) = update_runtime_health(
                storage,
                &report,
                RuntimeStatus::Unhealthy,
                "runtime.worker.failed",
                error.to_string(),
            ) {
                eprintln!("failed to write runtime worker failure health update: {health_error}");
            }
            return Err(error);
        }

        if let Some(max_heartbeats) = config.runtime_loop.max_heartbeats {
            if heartbeat_count >= max_heartbeats {
                let stopped = shutdown_runtime(config, storage)?;
                println!("{}", stopped.to_json_pretty()?);
                return Ok(());
            }
        }

        thread::sleep(cadence.worker_loop_interval());
        if runtime_stop_requested(storage)? {
            return Ok(());
        }

        if cadence.should_emit_heartbeat(last_heartbeat_at.elapsed()) {
            heartbeat_count += 1;
            report = update_runtime_health(
                storage,
                &report,
                RuntimeStatus::Healthy,
                RUNTIME_HEARTBEAT_EVENT,
                format!(
                    "heartbeat {} on supervision attempt {}",
                    heartbeat_count, attempt
                ),
            )?;
            last_heartbeat_at = Instant::now();

            if should_force_recoverable_crash(config, attempt, heartbeat_count) {
                let _ = update_runtime_health(
                    storage,
                    &report,
                    RuntimeStatus::Unhealthy,
                    "runtime.crash.recoverable",
                    format!(
                        "forcing recoverable crash after {} heartbeats on attempt {}",
                        heartbeat_count, attempt
                    ),
                )?;
                process::exit(config.supervision.recoverable_exit_code);
            }
        }
    }
}

pub(crate) fn runtime_stop_requested(storage: &StorageLayout) -> io::Result<bool> {
    match read_health_report(&storage.health_report_path) {
        Ok(report) => Ok(matches!(
            report.status,
            RuntimeStatus::ShuttingDown | RuntimeStatus::Stopped
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub(crate) fn supervise_runtime(
    config: &RuntimeConfig,
    storage: &StorageLayout,
) -> Result<(), Box<dyn Error>> {
    let executable = env::current_exe()?;
    let restart_policy = RuntimeRestartPolicy::from_settings(&config.supervision);

    let snapshot = bootstrap_runtime(config, storage, &executable, restart_policy.clone())?;
    let coordinator = LocalCoordinator::new(storage);
    recover_interrupted_build_attempts(&coordinator, &snapshot.recovery_report);

    let supervisor_process_id = process::id();
    let mut attempt = 1_u32;
    let mut restart_count = 0_u32;

    loop {
        write_supervisor_snapshot(
            storage,
            &RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                None,
                RuntimeSupervisorStatus::Starting,
                format!("spawning runtime serve attempt {attempt}"),
            )?,
        )?;

        let mut child = Command::new(&executable)
            .arg("serve")
            .env(SUPERVISION_ATTEMPT_ENV, attempt.to_string())
            .spawn()?;

        write_supervisor_snapshot(
            storage,
            &RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                Some(child.id()),
                attempt,
                restart_count,
                None,
                RuntimeSupervisorStatus::Running,
                format!("runtime serve attempt {attempt} running as pid {}", child.id()),
            )?,
        )?;

        let exit_status = child.wait()?;
        let exit_code = exit_status.code();

        if exit_status.success() {
            let snapshot = RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                exit_code,
                RuntimeSupervisorStatus::Completed,
                format!("runtime serve attempt {attempt} completed cleanly"),
            )?;
            write_supervisor_snapshot(storage, &snapshot)?;
            println!("{}", snapshot.to_json_pretty()?);
            return Ok(());
        }

        if restart_policy.should_restart(exit_code, restart_count) {
            restart_count += 1;
            let snapshot = RuntimeSupervisorSnapshot::new(
                config,
                supervisor_process_id,
                None,
                attempt,
                restart_count,
                exit_code,
                RuntimeSupervisorStatus::Restarting,
                format!(
                    "recoverable exit {:?} detected, restarting after {} ms",
                    exit_code,
                    restart_policy.restart_backoff_millis
                ),
            )?;
            write_supervisor_snapshot(storage, &snapshot)?;
            thread::sleep(Duration::from_millis(
                restart_policy.restart_backoff_millis,
            ));
            attempt += 1;
            continue;
        }

        let snapshot = RuntimeSupervisorSnapshot::new(
            config,
            supervisor_process_id,
            None,
            attempt,
            restart_count,
            exit_code,
            RuntimeSupervisorStatus::Failed,
            format!("runtime serve exited unsuccessfully with code {:?}", exit_code),
        )?;
        write_supervisor_snapshot(storage, &snapshot)?;
        if let Ok(report) = read_health_report(&storage.health_report_path) {
            let _ = update_runtime_health(
                storage,
                &report,
                RuntimeStatus::Unhealthy,
                "runtime.supervisor.failed",
                format!(
                    "supervisor exhausted restart policy after exit code {:?}",
                    exit_code
                ),
            )?;
        }
        return Err(format!("runtime serve exited unsuccessfully with code {:?}", exit_code).into());
    }
}

fn current_supervision_attempt() -> u32 {
    env::var(SUPERVISION_ATTEMPT_ENV)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|attempt| *attempt > 0)
        .unwrap_or(1)
}

fn should_force_recoverable_crash(
    config: &RuntimeConfig,
    attempt: u32,
    heartbeat_count: u32,
) -> bool {
    match config.runtime_loop.crash_after_heartbeats {
        Some(crash_after_heartbeats)
            if config.runtime_loop.crash_attempts >= attempt
                && heartbeat_count >= crash_after_heartbeats =>
        {
            true
        }
        _ => false,
    }
}
