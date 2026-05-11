use std::error::Error;
use std::fmt;

/// Models the durable lifecycle states stored for one release run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseStatus {
    Detected,
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl ReleaseStatus {
    /// Returns the SQLite label used by the current runtime schema.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// Returns whether the release is already in a terminal state.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }

    /// Queues a release run for local dispatch regardless of its prior state.
    ///
    /// The existing SQLite update does not guard the previous status, so the
    /// modeled transition intentionally collapses any known state into `queued`.
    pub const fn queue_for_dispatch(self) -> Self {
        let _ = self;
        Self::Queued
    }

    /// Mirrors the current manual rebuild behavior after derived execution state is cleared.
    pub const fn reset_after_rebuild(self) -> Self {
        let _ = self;
        Self::Detected
    }

    /// Enforces the queued-state precondition required before build planning.
    pub fn ensure_can_plan_builds(self) -> Result<(), LifecycleError> {
        if matches!(self, Self::Queued) {
            return Ok(());
        }

        Err(LifecycleError::ReleaseBuildPlanningRequiresQueued(self))
    }
}

/// Models the durable lifecycle states stored for one build run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl BuildStatus {
    /// Returns the SQLite label used by the current runtime schema.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// Returns whether the build run is already terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }

    /// Returns whether build planning may still rewrite this queued row.
    pub const fn accepts_plan_refresh(self) -> bool {
        matches!(self, Self::Queued)
    }

    /// Returns whether the build run still blocks repository-local sequencing.
    pub const fn blocks_repository_queue(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    /// Claims one queued build run into the running state.
    pub fn start(self) -> Result<Self, LifecycleError> {
        if matches!(self, Self::Queued) {
            return Ok(Self::Running);
        }

        Err(LifecycleError::BuildStartRequiresQueued(self))
    }

    /// Marks one running build run as succeeded.
    pub fn complete(self) -> Result<Self, LifecycleError> {
        if matches!(self, Self::Running) {
            return Ok(Self::Succeeded);
        }

        Err(LifecycleError::BuildCompletionRequiresRunning(self))
    }

    /// Marks one running build run as failed with a required terminal message.
    pub fn fail(self, error_message: &str) -> Result<Self, LifecycleError> {
        require_failure_message(error_message, "build")?;

        if matches!(self, Self::Running) {
            return Ok(Self::Failed);
        }

        Err(LifecycleError::BuildFailureRequiresRunning(self))
    }

    /// Marks one running build run as canceled with a required terminal message.
    pub fn cancel(self, error_message: &str) -> Result<Self, LifecycleError> {
        require_failure_message(error_message, "build")?;

        if matches!(self, Self::Running) {
            return Ok(Self::Canceled);
        }

        Err(LifecycleError::BuildFailureRequiresRunning(self))
    }
}

/// Models the durable lifecycle states stored for one publish run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublishStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl PublishStatus {
    /// Returns the SQLite label used by the current runtime schema.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// Returns whether the publish run is already terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }

    /// Claims one queued publish run into the running state.
    pub fn start(self) -> Result<Self, LifecycleError> {
        if matches!(self, Self::Queued) {
            return Ok(Self::Running);
        }

        Err(LifecycleError::PublishStartRequiresQueued(self))
    }

    /// Marks one running publish run as succeeded.
    pub fn complete(self) -> Result<Self, LifecycleError> {
        if matches!(self, Self::Running) {
            return Ok(Self::Succeeded);
        }

        Err(LifecycleError::PublishCompletionRequiresRunning(self))
    }

    /// Marks one running publish run as failed with a required terminal message.
    pub fn fail(self, error_message: &str) -> Result<Self, LifecycleError> {
        require_failure_message(error_message, "publish")?;

        if matches!(self, Self::Running) {
            return Ok(Self::Failed);
        }

        Err(LifecycleError::PublishFailureRequiresRunning(self))
    }
}

/// Reports invalid domain lifecycle transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    ReleaseBuildPlanningRequiresQueued(ReleaseStatus),
    BuildStartRequiresQueued(BuildStatus),
    BuildCompletionRequiresRunning(BuildStatus),
    BuildFailureRequiresRunning(BuildStatus),
    PublishStartRequiresQueued(PublishStatus),
    PublishCompletionRequiresRunning(PublishStatus),
    PublishFailureRequiresRunning(PublishStatus),
    MissingFailureMessage(&'static str),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReleaseBuildPlanningRequiresQueued(status) => write!(
                formatter,
                "release build planning requires queued status, got {}",
                status.as_str()
            ),
            Self::BuildStartRequiresQueued(status) => write!(
                formatter,
                "build run start requires queued status, got {}",
                status.as_str()
            ),
            Self::BuildCompletionRequiresRunning(status) => write!(
                formatter,
                "build run completion requires running status, got {}",
                status.as_str()
            ),
            Self::BuildFailureRequiresRunning(status) => write!(
                formatter,
                "build run failure requires running status, got {}",
                status.as_str()
            ),
            Self::PublishStartRequiresQueued(status) => write!(
                formatter,
                "publish run start requires queued status, got {}",
                status.as_str()
            ),
            Self::PublishCompletionRequiresRunning(status) => write!(
                formatter,
                "publish run completion requires running status, got {}",
                status.as_str()
            ),
            Self::PublishFailureRequiresRunning(status) => write!(
                formatter,
                "publish run failure requires running status, got {}",
                status.as_str()
            ),
            Self::MissingFailureMessage(kind) => {
                write!(formatter, "{kind} failure requires a non-empty error message")
            }
        }
    }
}

impl Error for LifecycleError {}

/// Reports whether one repository still has queued or running build work.
pub fn repository_build_process_active(statuses: &[BuildStatus]) -> bool {
    statuses.iter().copied().any(BuildStatus::blocks_repository_queue)
}

/// Reports whether one queued release still blocks repository-local execution.
pub fn release_needs_attention(statuses: &[BuildStatus]) -> bool {
    statuses.is_empty() || repository_build_process_active(statuses)
}

fn require_failure_message(
    error_message: &str,
    kind: &'static str,
) -> Result<(), LifecycleError> {
    if error_message.trim().is_empty() {
        return Err(LifecycleError::MissingFailureMessage(kind));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        release_needs_attention, repository_build_process_active, BuildStatus,
        LifecycleError, PublishStatus, ReleaseStatus,
    };

    #[test]
    fn release_queueing_and_rebuild_follow_current_store_behavior() {
        let statuses = [
            ReleaseStatus::Detected,
            ReleaseStatus::Queued,
            ReleaseStatus::Running,
            ReleaseStatus::Succeeded,
            ReleaseStatus::Failed,
            ReleaseStatus::Canceled,
        ];

        for status in statuses {
            assert_eq!(status.queue_for_dispatch(), ReleaseStatus::Queued);
            assert_eq!(status.reset_after_rebuild(), ReleaseStatus::Detected);
        }
    }

    #[test]
    fn build_planning_requires_a_queued_release() {
        assert!(ReleaseStatus::Queued.ensure_can_plan_builds().is_ok());

        for status in [
            ReleaseStatus::Detected,
            ReleaseStatus::Running,
            ReleaseStatus::Succeeded,
            ReleaseStatus::Failed,
            ReleaseStatus::Canceled,
        ] {
            assert_eq!(
                status.ensure_can_plan_builds(),
                Err(LifecycleError::ReleaseBuildPlanningRequiresQueued(status))
            );
        }
    }

    #[test]
    fn build_run_transitions_enforce_runtime_guards() {
        assert_eq!(BuildStatus::Queued.start(), Ok(BuildStatus::Running));
        assert_eq!(
            BuildStatus::Running.complete(),
            Ok(BuildStatus::Succeeded)
        );
        assert_eq!(BuildStatus::Running.fail("boom"), Ok(BuildStatus::Failed));
        assert_eq!(BuildStatus::Running.cancel("boom"), Ok(BuildStatus::Canceled));

        assert_eq!(
            BuildStatus::Running.start(),
            Err(LifecycleError::BuildStartRequiresQueued(
                BuildStatus::Running,
            ))
        );
        assert_eq!(
            BuildStatus::Queued.complete(),
            Err(LifecycleError::BuildCompletionRequiresRunning(
                BuildStatus::Queued,
            ))
        );
        assert_eq!(
            BuildStatus::Queued.fail("boom"),
            Err(LifecycleError::BuildFailureRequiresRunning(
                BuildStatus::Queued,
            ))
        );
        assert_eq!(
            BuildStatus::Queued.cancel("boom"),
            Err(LifecycleError::BuildFailureRequiresRunning(
                BuildStatus::Queued,
            ))
        );
        assert_eq!(
            BuildStatus::Running.fail("   "),
            Err(LifecycleError::MissingFailureMessage("build"))
        );
    }

    #[test]
    fn build_status_helpers_match_repository_queue_rules() {
        assert!(BuildStatus::Queued.accepts_plan_refresh());
        assert!(!BuildStatus::Running.accepts_plan_refresh());
        assert!(!BuildStatus::Succeeded.accepts_plan_refresh());

        assert!(BuildStatus::Queued.blocks_repository_queue());
        assert!(BuildStatus::Running.blocks_repository_queue());
        assert!(!BuildStatus::Succeeded.blocks_repository_queue());
        assert!(!BuildStatus::Failed.blocks_repository_queue());
        assert!(!BuildStatus::Canceled.blocks_repository_queue());

        assert!(repository_build_process_active(&[BuildStatus::Queued]));
        assert!(repository_build_process_active(&[
            BuildStatus::Succeeded,
            BuildStatus::Running,
        ]));
        assert!(!repository_build_process_active(&[
            BuildStatus::Succeeded,
            BuildStatus::Failed,
            BuildStatus::Canceled,
        ]));
    }

    #[test]
    fn queued_release_attention_matches_automation_reporting() {
        assert!(release_needs_attention(&[]));
        assert!(release_needs_attention(&[BuildStatus::Queued]));
        assert!(release_needs_attention(&[
            BuildStatus::Succeeded,
            BuildStatus::Running,
        ]));
        assert!(!release_needs_attention(&[
            BuildStatus::Succeeded,
            BuildStatus::Failed,
            BuildStatus::Canceled,
        ]));
    }

    #[test]
    fn publish_run_transitions_enforce_runtime_guards() {
        assert_eq!(PublishStatus::Queued.start(), Ok(PublishStatus::Running));
        assert_eq!(
            PublishStatus::Running.complete(),
            Ok(PublishStatus::Succeeded)
        );
        assert_eq!(
            PublishStatus::Running.fail("boom"),
            Ok(PublishStatus::Failed)
        );

        assert_eq!(
            PublishStatus::Running.start(),
            Err(LifecycleError::PublishStartRequiresQueued(
                PublishStatus::Running,
            ))
        );
        assert_eq!(
            PublishStatus::Queued.complete(),
            Err(LifecycleError::PublishCompletionRequiresRunning(
                PublishStatus::Queued,
            ))
        );
        assert_eq!(
            PublishStatus::Queued.fail("boom"),
            Err(LifecycleError::PublishFailureRequiresRunning(
                PublishStatus::Queued,
            ))
        );
        assert_eq!(
            PublishStatus::Running.fail("  "),
            Err(LifecycleError::MissingFailureMessage("publish"))
        );
    }
}